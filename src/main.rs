mod manager;
mod model;
mod protocol;

use axum::{
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use manager::App;
use model::Server;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    version,
    about = "Your Minecraft servers, in one place. A standalone Rust dashboard."
)]
struct Options {
    /// Configuration JSON; defaults to the OS config directory / void.dash / config.json
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long, default_value_t = 8787)]
    port: u16,
    /// Run as a remote management agent (requires VOID_DASH_TOKEN, minimum 32 characters)
    #[arg(long)]
    agent: bool,
    /// Agents may bind a private network interface; dashboards always bind loopback
    #[arg(long, default_value = "127.0.0.1")]
    bind: IpAddr,
    /// Don't open the system browser
    #[arg(long)]
    no_open: bool,
}

type ApiResult<T> = Result<Json<T>, ApiError>;
struct ApiError(String);
impl From<String> for ApiError {
    fn from(e: String) -> Self {
        Self(e)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = if self.0 == "Server not found" {
            StatusCode::NOT_FOUND
        } else {
            StatusCode::BAD_REQUEST
        };
        (status, Json(json!({"error":self.0}))).into_response()
    }
}
fn secure_equal(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
async fn guard(State(app): State<App>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    if !app.agent {
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let hostname = host.split(':').next().unwrap_or("");
        if !["localhost", "127.0.0.1"].contains(&hostname) {
            return (
                StatusCode::FORBIDDEN,
                Json(json!({"error":"Dashboard requests must use localhost or 127.0.0.1."})),
            )
                .into_response();
        }
        if let Some(origin) = headers.get(header::ORIGIN) {
            if origin.to_str().ok() != Some(format!("http://{host}").as_str()) {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({"error":"Cross-origin requests are not allowed."})),
                )
                    .into_response();
            }
        }
    }
    if request.uri().path().starts_with("/api/") {
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let cookie = if !app.agent {
            headers
                .get(header::COOKIE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| {
                    v.split(';')
                        .find_map(|c| c.trim().strip_prefix("void_session="))
                })
        } else {
            None
        };
        if !bearer
            .or(cookie)
            .is_some_and(|value| secure_equal(value, &app.token))
        {
            return (StatusCode::UNAUTHORIZED,Json(json!({"error":"Authentication required. Reopen the dashboard or check the agent token."}))).into_response();
        }
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; frame-src http: https:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'".parse().unwrap());
    response
}
async fn index(State(app): State<App>) -> Response {
    if app.agent {
        return (
            StatusCode::FORBIDDEN,
            "This is a void.dash agent. Connect it from your local dashboard.",
        )
            .into_response();
    }
    let mut response = Html(include_str!("../web/index.html")).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        format!(
            "void_session={}; HttpOnly; SameSite=Strict; Path=/",
            app.token
        )
        .parse()
        .unwrap(),
    );
    response
}
async fn servers(State(app): State<App>) -> Json<Value> {
    let servers = app
        .config
        .lock()
        .await
        .servers
        .iter()
        .map(Server::public)
        .collect::<Vec<_>>();
    let snapshots = app.snapshots.read().await;
    Json(json!({"servers":servers,"states":*snapshots}))
}
async fn add_server(State(app): State<App>, Json(server): Json<Server>) -> ApiResult<Value> {
    Ok(Json(app.save(server, None).await?.public()))
}
async fn edit_server(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(server): Json<Server>,
) -> ApiResult<Value> {
    Ok(Json(app.save(server, Some(&id)).await?.public()))
}
async fn remove_server(State(app): State<App>, Path(id): Path<String>) -> ApiResult<Value> {
    app.remove(&id).await?;
    Ok(Json(json!({"ok":true})))
}
async fn state(State(app): State<App>, Path(id): Path<String>) -> ApiResult<Value> {
    app.server(&id).await?;
    let snapshot = app
        .snapshots
        .read()
        .await
        .get(&id)
        .cloned()
        .unwrap_or_default();
    Ok(Json(serde_json::to_value(snapshot).unwrap()))
}
async fn logs(State(app): State<App>, Path(id): Path<String>) -> ApiResult<Value> {
    let server = app.server(&id).await?;
    if server.mode == "agent" {
        return Ok(Json(
            app.proxy(&server, reqwest::Method::GET, "logs", None)
                .await?,
        ));
    }
    Ok(Json(
        json!({"logs":app.logs.read().await.get(&id).cloned().unwrap_or_default()}),
    ))
}
async fn action(
    State(app): State<App>,
    Path((id, action)): Path<(String, String)>,
) -> ApiResult<Value> {
    let server = app.server(&id).await?;
    match action.as_str() {
        "start" => app.start(&server).await?,
        "stop" => app.stop(&server).await?,
        "restart" => {
            if server.mode == "rcon" {
                return Err(ApiError("Restarting remote servers requires an agent; RCON cannot start a stopped process.".into()));
            }
            if server.mode == "agent" {
                app.proxy(&server, reqwest::Method::POST, "action/restart", None)
                    .await?;
            } else {
                app.stop(&server).await?;
                app.start(&server).await?;
            }
        }
        _ => return Err(ApiError("Unknown action".into())),
    }
    Ok(Json(json!({"ok":true})))
}
#[derive(Deserialize)]
struct CommandInput {
    command: String,
}
async fn command(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<CommandInput>,
) -> ApiResult<Value> {
    let server = app.server(&id).await?;
    let output = app.command(&server, &input.command).await?;
    Ok(Json(json!({"output":output})))
}
async fn commands(State(app): State<App>, Path(id): Path<String>) -> ApiResult<Value> {
    let server = app.server(&id).await?;
    let commands = app.discover(&server).await?;
    Ok(Json(json!({"commands":commands})))
}
#[derive(Deserialize)]
struct ViewerInput {
    viewers: Vec<model::Viewer>,
}
async fn viewers(
    State(app): State<App>,
    Path(id): Path<String>,
    Json(input): Json<ViewerInput>,
) -> ApiResult<Value> {
    app.save_viewers(&id, input.viewers).await?;
    Ok(Json(json!({"ok":true})))
}
fn routes(app: App) -> Router {
    Router::new()
        .route("/", get(index))
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route("/api/servers", get(servers).post(add_server))
        .route(
            "/api/servers/{id}",
            axum::routing::put(edit_server).delete(remove_server),
        )
        .route("/api/servers/{id}/state", get(state))
        .route("/api/servers/{id}/logs", get(logs))
        .route("/api/servers/{id}/action/{action}", post(action))
        .route("/api/servers/{id}/command", post(command))
        .route("/api/servers/{id}/commands/refresh", post(commands))
        .route("/api/servers/{id}/viewers", axum::routing::put(viewers))
        .layer(DefaultBodyLimit::max(256 * 1024))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse();
    if !options.agent && options.bind != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err("The dashboard binds to 127.0.0.1. Use --agent for remote hosts.".into());
    }
    let token = if options.agent {
        let token = std::env::var("VOID_DASH_TOKEN")
            .map_err(|_| "Agent mode requires VOID_DASH_TOKEN (32+ characters).")?;
        if token.len() < 32 {
            return Err("VOID_DASH_TOKEN must be at least 32 characters.".into());
        }
        token
    } else {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    };
    let path = options.config.unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("void.dash/config.json")
    });
    let path = if path.is_relative() {
        std::env::current_dir()?.join(path)
    } else {
        path
    };
    let app = manager::Manager::new(path.clone(), token, options.agent).await?;
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(options.bind, options.port)).await?;
    let address = listener.local_addr()?;
    println!(
        "void.dash {} · {}",
        env!("CARGO_PKG_VERSION"),
        if options.agent { "agent" } else { "dashboard" }
    );
    println!("Configuration: {}", path.display());
    println!("Listening on http://{address}");
    let runner = app.clone();
    tokio::spawn(runner.background());
    if !options.no_open && !options.agent {
        let _ = open::that(format!("http://{address}"));
    }
    axum::serve(listener, routes(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    // Servers are not implicitly stopped; stop them in the dashboard before exiting.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_comparison() {
        assert!(secure_equal("token", "token"));
        assert!(!secure_equal("token", "other"));
        assert!(!secure_equal("token", ""));
    }
    #[tokio::test]
    async fn agent_requires_bearer_and_proxies_state_and_console_output() {
        let directory =
            std::env::temp_dir().join(format!("void-agent-test-{}", uuid::Uuid::new_v4()));
        let agent = manager::Manager::new(
            directory.join("agent.json"),
            "a-long-test-agent-token-of-32-characters".into(),
            true,
        )
        .await
        .unwrap();
        let server: Server = serde_json::from_value(
            json!({"name":"Remote world","host":"localhost","mode":"local","directory":"/tmp"}),
        )
        .unwrap();
        let remote = agent.save(server, None).await.unwrap();
        let snapshot = model::Snapshot {
            online: true,
            motd: "A remote world".into(),
            ..Default::default()
        };
        agent
            .snapshots
            .write()
            .await
            .insert(remote.id.clone(), snapshot);
        agent.log(&remote.id, "Remote console output".into()).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let service = tokio::spawn(async move {
            axum::serve(listener, routes(agent)).await.unwrap();
        });
        let client = reqwest::Client::new();
        assert_eq!(
            client
                .get(format!("{base}/api/servers"))
                .header(
                    header::COOKIE,
                    "void_session=a-long-test-agent-token-of-32-characters"
                )
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let dashboard =
            manager::Manager::new(directory.join("dashboard.json"), "local".into(), false)
                .await
                .unwrap();
        let server: Server = serde_json::from_value(json!({"name":"Remote","host":"localhost","mode":"agent","agent_url":base,"agent_token":"a-long-test-agent-token-of-32-characters","agent_server_id":remote.id})).unwrap();
        let state = dashboard
            .proxy(&server, reqwest::Method::GET, "state", None)
            .await
            .unwrap();
        assert_eq!(state["motd"], "A remote world");
        let logs = dashboard
            .proxy(&server, reqwest::Method::GET, "logs", None)
            .await
            .unwrap();
        assert_eq!(logs["logs"][0]["text"], "Remote console output");
        let mut invalid = server.clone();
        invalid.agent_token = "invalid".into();
        assert!(dashboard
            .proxy(&invalid, reqwest::Method::GET, "state", None)
            .await
            .is_err());
        service.abort();
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
    #[tokio::test]
    async fn api_requires_session_rejects_origins_and_persists_redacted_config() {
        let directory = std::env::temp_dir().join(format!("void-test-{}", uuid::Uuid::new_v4()));
        let app = manager::Manager::new(directory.join("config.json"), "test-token".into(), false)
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let service = tokio::spawn(async move {
            axum::serve(listener, routes(app)).await.unwrap();
        });
        let client = reqwest::Client::new();
        assert_eq!(
            client
                .get(format!("{base}/api/servers"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let cookie = client.get(&base).send().await.unwrap().headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        assert_eq!(
            client
                .get(format!("{base}/api/servers"))
                .header(header::COOKIE, &cookie)
                .header(header::ORIGIN, "https://attacker.invalid")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let server =
            json!({"name":"Test", "host":"127.0.0.1", "mode":"rcon", "rcon_password":"private"});
        let response = client
            .post(format!("{base}/api/servers"))
            .header(header::COOKIE, &cookie)
            .json(&server)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["rcon_password"], "");
        let id = body["id"].as_str().unwrap();
        let saved: Value = serde_json::from_slice(
            &tokio::fs::read(directory.join("config.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved["servers"][0]["rcon_password"], "private");
        assert!(client
            .delete(format!("{base}/api/servers/{id}"))
            .header(header::COOKIE, &cookie)
            .send()
            .await
            .unwrap()
            .status()
            .is_success());
        service.abort();
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}
