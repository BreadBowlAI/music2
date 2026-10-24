//! The one route table (AGENT-8): every HTTP route of the app, declared once, with its method, its
//! path, whether it writes the open song, whether agents may call it, plain words and an example
//! body. Each `routes*.rs` declares its routes with [`routes!`], which makes that file's router and
//! its part of the table from the same lines, so neither can drift from the other; [`all`] joins
//! the parts. The read-only window's 423 (WRITE-17) and the tools key's 403 (AGENT-11) are one
//! middleware, [`gate`], that reads it, and `m2 routes` prints it.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::AppState;

/// One route, as its file declares it.
#[derive(Debug)]
pub struct Route {
    /// `GET`, `POST` or `PUT`.
    pub method: &'static str,
    pub path: &'static str,
    /// It writes the open song, or begins a drag whose commit would: a window open read-only
    /// answers it 423 (WRITE-17).
    pub writes: bool,
    /// Agents may call it, with the tools key. Otherwise it is the musician's, and the tools key
    /// gets 403 (AGENT-11).
    pub agents: bool,
    /// What it does, in plain words.
    pub words: &'static str,
    /// An example body, a JSON object: a GET's keys go in its query.
    pub example: &'static str,
}

impl Route {
    /// The example body, as JSON.
    pub fn example(&self) -> Value {
        serde_json::from_str(self.example).unwrap_or(Value::Null)
    }

    /// The route as `m2 routes --json` prints it.
    pub fn json(&self) -> Value {
        json!({
            "method": self.method,
            "path": self.path,
            "writes": self.writes,
            "agents": self.agents,
            "words": self.words,
            "example": self.example(),
        })
    }

    /// What the app answers an agent that calls a route not for agents, and what `m2 call` says
    /// before it sends one (AGENT-11).
    pub fn refusal(&self) -> String {
        let mut words = self.words.chars();
        let first = words.next().into_iter();
        // "Play or stop" reads "play or stop" in the sentence; "MIDI…" stays.
        let lower = match (words.clone().next(), self.words.chars().next()) {
            (Some(next), Some(_)) if next.is_lowercase() => first.flat_map(char::to_lowercase).chain(words).collect::<String>(),
            _ => self.words.to_string(),
        };
        format!(
            "{} {} is the musician's: {lower}. Agents can't call it (AGENT-11); ask the musician, or point at it with m2 point.",
            self.method, self.path
        )
    }
}

/// Every file's part of the table.
const TABLES: &[&[Route]] = &[
    crate::routes::ROUTES,
    crate::routes_8b::ROUTES,
    crate::routes_audio::ROUTES,
    crate::routes_exchange::ROUTES,
    crate::routes_fx::ROUTES,
    crate::routes_leases::ROUTES,
    crate::routes_mix::ROUTES,
    crate::routes_notes::ROUTES,
    crate::routes_plugins::ROUTES,
    crate::routes_presets::ROUTES,
    crate::routes_project::ROUTES,
    crate::routes_record::ROUTES,
    crate::routes_see::ROUTES,
    crate::routes_settings::ROUTES,
    crate::routes_view::ROUTES,
];

/// Every route, by path, then method: the one table.
pub fn all() -> Vec<&'static Route> {
    let mut all: Vec<&'static Route> = TABLES.iter().flat_map(|t| t.iter()).collect();
    all.sort_by_key(|r| (r.path, r.method));
    all
}

/// The route `method path` names, if the table has it. A HEAD is a GET's.
pub fn find(method: &str, path: &str) -> Option<&'static Route> {
    let method = if method == "HEAD" { "GET" } else { method };
    TABLES.iter().flat_map(|t| t.iter()).find(|r| r.method == method && r.path == path)
}

/// The routes of the table, each file's router made from its part, and a clear 404 for any other
/// `/api` path, not the page's index.html.
pub(crate) fn router() -> axum::Router<Arc<AppState>> {
    axum::Router::new()
        .merge(crate::routes::router())
        .merge(crate::routes_8b::router())
        .merge(crate::routes_audio::router())
        .merge(crate::routes_exchange::router())
        .merge(crate::routes_fx::router())
        .merge(crate::routes_leases::router())
        .merge(crate::routes_mix::router())
        .merge(crate::routes_notes::router())
        .merge(crate::routes_plugins::router())
        .merge(crate::routes_presets::router())
        .merge(crate::routes_project::router())
        .merge(crate::routes_record::router())
        .merge(crate::routes_see::router())
        .merge(crate::routes_settings::router())
        .merge(crate::routes_view::router())
        .route("/api/{*rest}", axum::routing::any(no_such_api))
}

async fn no_such_api() -> Response {
    (StatusCode::NOT_FOUND, Json(json!({"error": "no such API"}))).into_response()
}

/// The table's gate, inside `auth`, which says who asks: the tools key on a route not for agents
/// is 403, with the route's refusal (AGENT-11); a write while the song is open read-only is 423,
/// with WRITE-18's message (WRITE-17). Anything else goes on; a path the table hasn't got, to the
/// router's 404.
pub(crate) async fn gate(State(s): State<Arc<AppState>>, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    if let Some(route) = find(req.method().as_str(), req.uri().path()) {
        if !route.agents && crate::caller::now().is_agent() {
            return (StatusCode::FORBIDDEN, Json(json!({"error": route.refusal()}))).into_response();
        }
        if route.writes
            && let Some(why) = s.graph.read_only()
        {
            return (StatusCode::LOCKED, Json(json!({"error": why}))).into_response();
        }
    }
    next.run(req).await
}

/// Declare a file's routes (AGENT-8), each as
///
/// ```text
/// METHOD "path" => handler [up to LIMIT]; writes|reads, agents|musician, "words", r#"example"#;
/// ```
///
/// `up to` raises the largest body it takes. It makes the file's `ROUTES`, its part of the one
/// table, and its `router()`, from the same lines.
macro_rules! routes {
    (@writes writes) => { true };
    (@writes reads) => { false };
    (@who agents) => { true };
    (@who musician) => { false };
    (@method GET $h:ident) => { axum::routing::get($h) };
    (@method POST $h:ident) => { axum::routing::post($h) };
    (@method PUT $h:ident) => { axum::routing::put($h) };
    ($($method:ident $path:literal => $handler:ident $(up to $limit:expr)?; $writes:ident, $who:ident, $words:literal, $example:literal;)*) => {
        /// This file's routes: its part of the one route table (AGENT-8).
        pub(crate) const ROUTES: &[$crate::api::Route] = &[$($crate::api::Route {
            method: stringify!($method),
            path: $path,
            writes: $crate::api::routes!(@writes $writes),
            agents: $crate::api::routes!(@who $who),
            words: $words,
            example: $example,
        }),*];

        /// This file's router, made from the same lines as [`ROUTES`].
        pub(crate) fn router() -> axum::Router<std::sync::Arc<$crate::AppState>> {
            axum::Router::new()$(.route($path, $crate::api::routes!(@method $method $handler)$(.layer(axum::extract::DefaultBodyLimit::max($limit)))?))*
        }
    };
}
pub(crate) use routes;

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
