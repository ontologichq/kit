//! A fake engine for testing clients: it serves the `Ontologic` service over TLS on a free port
//! of 127.0.0.1, answers each call from a script, and remembers every call it got.
//!
//! ```no_run
//! use ontologic_kit::fake::{FakeEngine, Rpc};
//! use ontologic_kit::pb;
//!
//! let engine = FakeEngine::start();
//! engine.sign_in("admin", "secret");
//! engine.reply(Rpc::Health, pb::HealthReply { llm: "fake-model".into(), ..Default::default() });
//! // Point a client at engine.host(), trusting engine.ca_pem().
//! ```

use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use prost::Message;
use tokio_stream::Stream;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Identity, Server, ServerTlsConfig};
use tonic::{Request, Response, Status};

use crate::pb;
use crate::pb::ontologic_server::{Ontologic, OntologicServer};
use crate::{KEY_HEADER, SIGN_IN_FAILED, USER_HEADER};

/// The calls of the `Ontologic` service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rpc {
    Health,
    ListTenants,
    CreateTenant,
    GetTenant,
    DeleteTenant,
    TenantMeta,
    Import,
    ImportBlob,
    Ask,
    Commit,
    Rollback,
    Show,
    ExportTurtle,
    ImportTurtle,
    Me,
    AddUser,
    Grant,
    Revoke,
    TenantUsers,
}

/// One call the fake got: which, who signed in, and the request message's bytes.
#[derive(Clone, Debug)]
pub struct Call {
    pub rpc: Rpc,
    pub user: String,
    pub key: String,
    body: Vec<u8>,
}

impl Call {
    /// The request message.
    pub fn request<M: Message + Default>(&self) -> M {
        M::decode(self.body.as_slice()).expect("the request decodes as the message asked for")
    }
}

/// A scripted answer: one message, a stream of messages, or an error.
enum Reply {
    One(Result<Vec<u8>, Status>),
    Stream(Vec<Result<Vec<u8>, Status>>),
}

#[derive(Default)]
struct Script {
    /// Replies in order per call; the last one repeats.
    replies: BTreeMap<Rpc, VecDeque<Reply>>,
    calls: Vec<Call>,
    sign_in: Option<(String, String)>,
}

impl Script {
    fn next(&mut self, rpc: Rpc) -> Result<Reply, Status> {
        let queue = self
            .replies
            .get_mut(&rpc)
            .filter(|q| !q.is_empty())
            .ok_or_else(|| {
                Status::unimplemented(format!("the fake engine has no reply for {rpc:?}"))
            })?;
        Ok(match queue.len() {
            1 => match &queue[0] {
                Reply::One(r) => Reply::One(r.clone()),
                Reply::Stream(events) => Reply::Stream(events.clone()),
            },
            _ => queue.pop_front().expect("not empty"),
        })
    }
}

/// A fake engine running on its own thread until dropped with the process.
#[derive(Clone)]
pub struct FakeEngine {
    script: Arc<Mutex<Script>>,
    host: String,
    ca_pem: String,
}

impl FakeEngine {
    /// Serves on a free port of 127.0.0.1 with a fresh self-signed certificate for localhost.
    pub fn start() -> FakeEngine {
        let generated = rcgen::generate_simple_self_signed(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ])
        .expect("generate a certificate");
        let (cert_pem, key_pem) = (generated.cert.pem(), generated.signing_key.serialize_pem());
        let script = Arc::new(Mutex::new(Script::default()));
        let service = Service {
            script: script.clone(),
        };
        let (bound, listening) = std::sync::mpsc::channel();
        let identity = Identity::from_pem(cert_pem.clone(), key_pem);
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().expect("a runtime for the fake engine");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("bind 127.0.0.1:0");
                bound
                    .send(listener.local_addr().expect("bound address"))
                    .expect("the starter waits");
                let served = Server::builder()
                    .tls_config(ServerTlsConfig::new().identity(identity))
                    .expect("TLS for the fake engine")
                    .add_service(OntologicServer::new(service))
                    .serve_with_incoming(TcpListenerStream::new(listener))
                    .await;
                if let Err(e) = served {
                    eprintln!("fake engine stopped: {e}");
                }
            });
        });
        let addr = listening.recv().expect("the fake engine binds");
        FakeEngine {
            script,
            host: format!("localhost:{}", addr.port()),
            ca_pem: cert_pem,
        }
    }

    /// `localhost:<port>`.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The certificate to trust, PEM.
    pub fn ca_pem(&self) -> &str {
        &self.ca_pem
    }

    /// Only calls signed in as `user` with `key` are answered; others get UNAUTHENTICATED with
    /// the engine's own message. Without this, any sign in is accepted.
    pub fn sign_in(&self, user: &str, key: &str) {
        self.lock().sign_in = Some((user.to_string(), key.to_string()));
    }

    /// Answers the next `rpc` with `message`. Replies are used in order; the last one repeats.
    pub fn reply<M: Message>(&self, rpc: Rpc, message: M) {
        self.push(rpc, Reply::One(Ok(message.encode_to_vec())));
    }

    /// Answers the next `rpc` with an error.
    pub fn fail(&self, rpc: Rpc, status: Status) {
        self.push(rpc, Reply::One(Err(status)));
    }

    /// Answers the next streaming `rpc` (Import, ImportBlob, Ask) with these events in order.
    pub fn stream<M: Message>(&self, rpc: Rpc, events: Vec<Result<M, Status>>) {
        let events = events
            .into_iter()
            .map(|e| e.map(|m| m.encode_to_vec()))
            .collect();
        self.push(rpc, Reply::Stream(events));
    }

    /// Every call so far, in the order they came.
    pub fn calls(&self) -> Vec<Call> {
        self.lock().calls.clone()
    }

    fn push(&self, rpc: Rpc, reply: Reply) {
        self.lock().replies.entry(rpc).or_default().push_back(reply);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Script> {
        self.script
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

struct Service {
    script: Arc<Mutex<Script>>,
}

type Events<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>;

impl Service {
    /// Checks the sign in, records the call and takes its scripted reply.
    fn take<Q: Message>(&self, rpc: Rpc, request: &Request<Q>) -> Result<Reply, Status> {
        let text = |name: &str| {
            request
                .metadata()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        let (user, key) = (text(USER_HEADER), text(KEY_HEADER));
        let mut script = self.script.lock().unwrap_or_else(|p| p.into_inner());
        script.calls.push(Call {
            rpc,
            user: user.clone(),
            key: key.clone(),
            body: request.get_ref().encode_to_vec(),
        });
        if let Some((want_user, want_key)) = &script.sign_in
            && (want_user != &user || want_key != &key)
        {
            return Err(Status::unauthenticated(SIGN_IN_FAILED));
        }
        script.next(rpc)
    }

    fn one<Q: Message, R: Message + Default>(
        &self,
        rpc: Rpc,
        request: Request<Q>,
    ) -> Result<Response<R>, Status> {
        match self.take(rpc, &request)? {
            Reply::One(reply) => Ok(Response::new(decode(&reply?)?)),
            Reply::Stream(_) => Err(Status::internal(format!(
                "{rpc:?} was scripted as a stream"
            ))),
        }
    }

    fn many<Q: Message, R: Message + Default + 'static>(
        &self,
        rpc: Rpc,
        request: Request<Q>,
    ) -> Result<Response<Events<R>>, Status> {
        let events = match self.take(rpc, &request)? {
            Reply::Stream(events) => events,
            Reply::One(Err(status)) => return Err(status),
            Reply::One(Ok(_)) => {
                return Err(Status::internal(format!(
                    "{rpc:?} was scripted as one message"
                )));
            }
        };
        let decoded: Vec<Result<R, Status>> = events
            .into_iter()
            .map(|e| e.and_then(|bytes| decode(&bytes)))
            .collect();
        Ok(Response::new(Box::pin(tokio_stream::iter(decoded))))
    }
}

fn decode<R: Message + Default>(bytes: &[u8]) -> Result<R, Status> {
    R::decode(bytes).map_err(|e| Status::internal(format!("scripted reply does not decode: {e}")))
}

#[tonic::async_trait]
impl Ontologic for Service {
    type ImportStream = Events<pb::ImportEvent>;
    type ImportBlobStream = Events<pb::ImportEvent>;
    type AskStream = Events<pb::AskEvent>;

    async fn health(&self, r: Request<pb::Empty>) -> Result<Response<pb::HealthReply>, Status> {
        self.one(Rpc::Health, r)
    }
    async fn list_tenants(
        &self,
        r: Request<pb::Empty>,
    ) -> Result<Response<pb::TenantList>, Status> {
        self.one(Rpc::ListTenants, r)
    }
    async fn create_tenant(
        &self,
        r: Request<pb::TenantName>,
    ) -> Result<Response<pb::TenantSummary>, Status> {
        self.one(Rpc::CreateTenant, r)
    }
    async fn get_tenant(
        &self,
        r: Request<pb::TenantName>,
    ) -> Result<Response<pb::TenantSummary>, Status> {
        self.one(Rpc::GetTenant, r)
    }
    async fn delete_tenant(
        &self,
        r: Request<pb::TenantName>,
    ) -> Result<Response<pb::TenantSummary>, Status> {
        self.one(Rpc::DeleteTenant, r)
    }
    async fn tenant_meta(&self, r: Request<pb::TenantName>) -> Result<Response<pb::Meta>, Status> {
        self.one(Rpc::TenantMeta, r)
    }
    async fn import(
        &self,
        r: Request<pb::ImportRequest>,
    ) -> Result<Response<Self::ImportStream>, Status> {
        self.many(Rpc::Import, r)
    }
    async fn import_blob(
        &self,
        r: Request<pb::BlobRequest>,
    ) -> Result<Response<Self::ImportBlobStream>, Status> {
        self.many(Rpc::ImportBlob, r)
    }
    async fn ask(&self, r: Request<pb::AskRequest>) -> Result<Response<Self::AskStream>, Status> {
        self.many(Rpc::Ask, r)
    }
    async fn commit(
        &self,
        r: Request<pb::TenantName>,
    ) -> Result<Response<pb::TenantSummary>, Status> {
        self.one(Rpc::Commit, r)
    }
    async fn rollback(
        &self,
        r: Request<pb::TenantName>,
    ) -> Result<Response<pb::TenantSummary>, Status> {
        self.one(Rpc::Rollback, r)
    }
    async fn show(&self, r: Request<pb::ShowRequest>) -> Result<Response<pb::ShowReply>, Status> {
        self.one(Rpc::Show, r)
    }
    async fn export_turtle(
        &self,
        r: Request<pb::TenantName>,
    ) -> Result<Response<pb::TurtleFile>, Status> {
        self.one(Rpc::ExportTurtle, r)
    }
    async fn import_turtle(
        &self,
        r: Request<pb::TurtleFile>,
    ) -> Result<Response<pb::TurtleImported>, Status> {
        self.one(Rpc::ImportTurtle, r)
    }
    async fn me(&self, r: Request<pb::Empty>) -> Result<Response<pb::User>, Status> {
        self.one(Rpc::Me, r)
    }
    async fn add_user(&self, r: Request<pb::UserName>) -> Result<Response<pb::NewUser>, Status> {
        self.one(Rpc::AddUser, r)
    }
    async fn grant(&self, r: Request<pb::Access>) -> Result<Response<pb::User>, Status> {
        self.one(Rpc::Grant, r)
    }
    async fn revoke(&self, r: Request<pb::Access>) -> Result<Response<pb::User>, Status> {
        self.one(Rpc::Revoke, r)
    }
    async fn tenant_users(
        &self,
        r: Request<pb::TenantName>,
    ) -> Result<Response<pb::UserList>, Status> {
        self.one(Rpc::TenantUsers, r)
    }
}
