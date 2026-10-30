//! The one route table (AGENT-8): every HTTP route of the app, declared once, with its method, its
//! path, the largest body it takes, whether it writes the open song, whether agents may call it,
//! plain words and an example body. Each `routes*.rs` declares its routes with [`routes!`], which
//! makes that file's router and its part of the table from the same lines, so neither can drift
//! from the other; [`all`] joins the parts. One middleware, [`gate`], answers from it: any other
//! `/api` path or method (404, 405), the tools key's 403 (AGENT-11) and the read-only window's 423
//! (WRITE-17). `m2 routes` prints it.

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
    /// The largest body it takes, in bytes: [`BODY`], unless its line says `up to`. The router
    /// refuses a bigger one, 413, and `m2 call` before sending it.
    pub limit: usize,
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

/// The largest body a route takes unless its line says more: 2 MiB, axum's own.
pub const BODY: usize = 2 << 20;

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
            "limit": self.limit,
            "writes": self.writes,
            "agents": self.agents,
            "words": self.words,
            "example": self.example(),
        })
    }

    /// What the app answers an agent that calls a route not for agents, and what `m2 call` says
    /// before it sends one (AGENT-11).
    pub fn refusal(&self) -> String {
        // "Play or stop" reads "play or stop" in the sentence; "MIDI…" stays as it is.
        let mut chars = self.words.chars();
        let lower: String = match (chars.next(), chars.clone().next()) {
            (Some(first), Some(next)) if next.is_lowercase() => first.to_lowercase().chain(chars).collect(),
            _ => self.words.to_string(),
        };
        format!(
            "{} {} is the musician's: {lower}. Agents can't call it (AGENT-11); ask the musician, or point at it with m2 point.",
            self.method, self.path
        )
    }
}

/// One file's routes, as [`routes!`] makes them from the same lines: its part of the table, and
/// its router.
pub(crate) struct Part {
    routes: &'static [Route],
    router: fn() -> axum::Router<Arc<AppState>>,
}

impl Part {
    pub(crate) const fn new(routes: &'static [Route], router: fn() -> axum::Router<Arc<AppState>>) -> Part {
        Part { routes, router }
    }
}

/// Every file's routes, each named once.
const PARTS: [Part; 17] = [
    crate::routes::PART,
    crate::routes_8b::PART,
    crate::routes_audio::PART,
    crate::routes_exchange::PART,
    crate::routes_fx::PART,
    crate::routes_leases::PART,
    crate::routes_library::PART,
    crate::routes_mix::PART,
    crate::routes_notes::PART,
    crate::routes_packs::PART,
    crate::routes_plugins::PART,
    crate::routes_presets::PART,
    crate::routes_project::PART,
    crate::routes_record::PART,
    crate::routes_see::PART,
    crate::routes_settings::PART,
    crate::routes_view::PART,
];

/// Every route, by path, then method: the one table.
pub fn all() -> Vec<&'static Route> {
    let mut all: Vec<&'static Route> = PARTS.iter().flat_map(|p| p.routes).collect();
    all.sort_by_key(|r| (r.path, r.method));
    all
}

/// The route `method path` names, if the table has it. A HEAD is a GET's.
pub fn find(method: &str, path: &str) -> Option<&'static Route> {
    let method = if method == "HEAD" { "GET" } else { method };
    PARTS.iter().flat_map(|p| p.routes).find(|r| r.method == method && r.path == path)
}

/// The routes of the table, each file's router made from its part.
pub(crate) fn router() -> axum::Router<Arc<AppState>> {
    PARTS.iter().fold(axum::Router::new(), |r, p| r.merge((p.router)()))
}

/// The table's gate, inside `auth`, which says who asks. An `/api` path the table hasn't got is
/// 404, and a method it hasn't got on a path it has 405, whoever asks: deny by default, so no
/// route outside the table answers (AGENT-8). Then the tools key on a route not for agents is 403,
/// with the route's refusal (AGENT-11); a write while the song is open read-only is 423, with
/// WRITE-18's message (WRITE-17). Any other path is the page's files.
pub(crate) async fn gate(State(s): State<Arc<AppState>>, req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let path = req.uri().path();
    if !path.starts_with("/api/") {
        return next.run(req).await;
    }
    let Some(route) = find(req.method().as_str(), path) else {
        let methods: Vec<&str> = all().into_iter().filter(|r| r.path == path).map(|r| r.method).collect();
        if methods.is_empty() {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "no such API"}))).into_response();
        }
        let takes = methods.join(" or ");
        let said = Json(json!({"error": format!("{path} takes {takes}, not {}", req.method())}));
        return (StatusCode::METHOD_NOT_ALLOWED, [(axum::http::header::ALLOW, methods.join(", "))], said).into_response();
    };
    if !route.agents && crate::caller::now().is_agent() {
        return (StatusCode::FORBIDDEN, Json(json!({"error": route.refusal()}))).into_response();
    }
    if route.writes
        && let Some(why) = s.graph.read_only()
    {
        return (StatusCode::LOCKED, Json(json!({"error": why}))).into_response();
    }
    next.run(req).await
}

/// Declare a file's routes (AGENT-8), each as
///
/// ```text
/// METHOD "path" => handler [up to LIMIT]; writes|reads, agents|musician, "words", r#"example"#;
/// ```
///
/// `up to` raises the largest body it takes from [`BODY`]. It makes the file's `PART`: its part of
/// the one table and its router, from the same lines, so the limit the router keeps is the table's.
macro_rules! routes {
    (@writes writes) => { true };
    (@writes reads) => { false };
    (@who agents) => { true };
    (@who musician) => { false };
    (@method GET $h:ident) => { axum::routing::get($h) };
    (@method POST $h:ident) => { axum::routing::post($h) };
    (@method PUT $h:ident) => { axum::routing::put($h) };
    (@limit) => { $crate::api::BODY };
    (@limit $limit:expr) => { $limit };
    ($($method:ident $path:literal => $handler:ident $(up to $limit:expr)?; $writes:ident, $who:ident, $words:literal, $example:literal;)*) => {
        /// This file's routes and its router, made from the same lines: its part of the one route
        /// table (AGENT-8).
        pub(crate) const PART: $crate::api::Part = $crate::api::Part::new(ROUTES, router);

        const ROUTES: &[$crate::api::Route] = &[$($crate::api::Route {
            method: stringify!($method),
            path: $path,
            limit: $crate::api::routes!(@limit $($limit)?),
            writes: $crate::api::routes!(@writes $writes),
            agents: $crate::api::routes!(@who $who),
            words: $words,
            example: $example,
        }),*];

        fn router() -> axum::Router<std::sync::Arc<$crate::AppState>> {
            axum::Router::new()$(.route(
                $path,
                $crate::api::routes!(@method $method $handler)
                    .layer(axum::extract::DefaultBodyLimit::max($crate::api::routes!(@limit $($limit)?))),
            ))*
        }
    };
}
pub(crate) use routes;

#[cfg(test)]
#[path = "api_tests.rs"]
mod tests;
