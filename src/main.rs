use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use clap::Parser;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

/// 웹 브라우저로 접속하는 간단한 파일 서버
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// 공유할 드라이브/폴더. `경로` 또는 `이름=경로` (예: D:\ , 문서=C:\Users\me\Documents)
    #[arg(required = true)]
    paths: Vec<String>,

    /// 수신 주소 (같은 네트워크의 다른 기기에서 접속하려면 0.0.0.0)
    #[arg(short, long, default_value = "0.0.0.0")]
    bind: IpAddr,

    /// 포트
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// 접속 사용자 이름 (--password 와 함께 지정하면 로그인 필요)
    #[arg(short, long, default_value = "admin")]
    user: String,

    /// 접속 암호. 지정하지 않으면 인증 없이 공유됩니다
    #[arg(short = 'P', long)]
    password: Option<String>,

    /// 브라우저에서 업로드 허용 (기본은 읽기 전용)
    #[arg(long)]
    upload: bool,
}

struct Share {
    name: String,
    root: PathBuf,
}

struct AppState {
    shares: Vec<Share>,
    /// 기대하는 Authorization 헤더 값 ("Basic xxx"). None 이면 인증 안 함
    auth: Option<String>,
    allow_upload: bool,
}

type ApiError = (StatusCode, String);

#[derive(Serialize)]
struct Entry {
    name: String,
    is_dir: bool,
    size: u64,
    modified: u64,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let mut shares: Vec<Share> = Vec::new();
    for spec in &args.paths {
        let (name, path) = match spec.split_once('=') {
            Some((n, p)) => (n.to_string(), p.to_string()),
            None => (String::new(), spec.clone()),
        };
        // 드라이브 루트("D:\")는 "D:\" 뒤에 붙은 따옴표 이스케이프 문제를 피하려고 끝의 `"` 도 제거
        let path = path.trim_end_matches('"');
        let root = match std::fs::canonicalize(path) {
            Ok(p) if p.is_dir() => p,
            _ => {
                eprintln!("폴더를 찾을 수 없습니다: {path}");
                std::process::exit(1);
            }
        };
        let name = if name.is_empty() { default_name(&root) } else { name };
        if shares.iter().any(|s| s.name == name) {
            eprintln!("공유 이름이 중복됩니다: {name} ('이름=경로' 형식으로 구분하세요)");
            std::process::exit(1);
        }
        shares.push(Share { name, root });
    }

    let auth = args.password.as_ref().map(|p| {
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", args.user, p));
        format!("Basic {token}")
    });

    let state = Arc::new(AppState { shares, auth, allow_upload: args.upload });

    let app = Router::new()
        .route("/", get(index))
        .route("/api/info", get(info))
        .route("/api/list/{share}", get(list))
        .route("/api/list/{share}/{*path}", get(list))
        .route("/files/{share}/{*path}", get(file))
        .route("/api/upload/{share}", post(upload))
        .route("/api/upload/{share}/{*path}", post(upload))
        .layer(DefaultBodyLimit::disable())
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
        .with_state(state.clone());

    let addr = SocketAddr::new(args.bind, args.port);
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{addr} 에서 수신을 시작하지 못했습니다: {e}");
            std::process::exit(1);
        }
    };

    println!("공유 목록:");
    for s in &state.shares {
        println!("  {} -> {}", s.name, s.root.display());
    }
    println!(
        "모드: {}, 인증: {}",
        if args.upload { "읽기/업로드" } else { "읽기 전용" },
        if state.auth.is_some() { "사용" } else { "없음 (누구나 접속 가능)" }
    );
    println!("접속 주소:");
    println!("  http://localhost:{}", args.port);
    if args.bind.is_unspecified() {
        if let Some(ip) = lan_ip() {
            println!("  http://{ip}:{}", args.port);
        }
    }
    println!("종료: Ctrl+C");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("서버 실행 실패");
}

fn default_name(root: &FsPath) -> String {
    if let Some(n) = root.file_name() {
        return n.to_string_lossy().into_owned();
    }
    // 드라이브 루트: \\?\D:\ -> "D"
    root.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .chars()
        .take_while(|c| c.is_alphanumeric())
        .collect()
}

/// 패킷을 보내지 않고 외부로 나가는 인터페이스의 주소만 알아낸다
fn lan_ip() -> Option<IpAddr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("8.8.8.8:80").ok()?;
    Some(sock.local_addr().ok()?.ip())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

async fn require_auth(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let Some(expected) = &state.auth else {
        return next.run(req).await;
    };
    let ok = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| constant_time_eq(v.as_bytes(), expected.as_bytes()));
    if ok {
        return next.run(req).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, r#"Basic realm="fileserver", charset="UTF-8""#)],
        "인증이 필요합니다",
    )
        .into_response()
}

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

async fn info(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let shares: Vec<&str> = state.shares.iter().map(|s| s.name.as_str()).collect();
    Json(serde_json::json!({ "shares": shares, "upload": state.allow_upload }))
}

/// 공유 이름과 상대 경로를 실제 경로로 변환한다.
/// `..`·드라이브 접두어·ADS(`:`)를 거부하고, 심볼릭 링크/정션으로 공유 밖을 가리키면 거부한다.
fn resolve(state: &AppState, share: &str, rel: &str) -> Result<PathBuf, ApiError> {
    let share = state
        .shares
        .iter()
        .find(|s| s.name == share)
        .ok_or((StatusCode::NOT_FOUND, "공유를 찾을 수 없습니다".to_string()))?;

    let mut full = share.root.clone();
    for comp in FsPath::new(rel).components() {
        match comp {
            Component::Normal(seg) => {
                if seg.to_string_lossy().contains(':') {
                    return Err((StatusCode::BAD_REQUEST, "잘못된 경로입니다".to_string()));
                }
                full.push(seg);
            }
            Component::CurDir => {}
            _ => return Err((StatusCode::BAD_REQUEST, "잘못된 경로입니다".to_string())),
        }
    }

    let canon = std::fs::canonicalize(&full)
        .map_err(|_| (StatusCode::NOT_FOUND, "찾을 수 없습니다".to_string()))?;
    if !canon.starts_with(&share.root) {
        return Err((StatusCode::FORBIDDEN, "공유 범위를 벗어난 경로입니다".to_string()));
    }
    Ok(canon)
}

fn params(p: &HashMap<String, String>) -> (&str, &str) {
    (
        p.get("share").map(String::as_str).unwrap_or(""),
        p.get("path").map(String::as_str).unwrap_or(""),
    )
}

async fn list(
    State(state): State<Arc<AppState>>,
    Path(p): Path<HashMap<String, String>>,
) -> Result<Json<Vec<Entry>>, ApiError> {
    let (share, rel) = params(&p);
    let dir = resolve(&state, share, rel)?;
    if !dir.is_dir() {
        return Err((StatusCode::BAD_REQUEST, "폴더가 아닙니다".to_string()));
    }

    let mut rd = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| (StatusCode::FORBIDDEN, format!("폴더를 읽지 못했습니다: {e}")))?;
    let mut out = Vec::new();
    while let Ok(Some(ent)) = rd.next_entry().await {
        // 권한이 없는 항목($RECYCLE.BIN 등)은 목록에서 뺀다
        let Ok(meta) = tokio::fs::metadata(ent.path()).await else {
            continue;
        };
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        out.push(Entry {
            name: ent.file_name().to_string_lossy().into_owned(),
            is_dir: meta.is_dir(),
            size: if meta.is_dir() { 0 } else { meta.len() },
            modified,
        });
    }
    Ok(Json(out))
}

async fn file(
    State(state): State<Arc<AppState>>,
    Path(p): Path<HashMap<String, String>>,
    Query(q): Query<HashMap<String, String>>,
    req: Request<Body>,
) -> Result<Response, ApiError> {
    let (share, rel) = params(&p);
    let path = resolve(&state, share, rel)?;
    if !path.is_file() {
        return Err((StatusCode::NOT_FOUND, "파일이 아닙니다".to_string()));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

    // Range / If-Modified-Since 등은 ServeFile 이 처리한다
    let mut resp = ServeFile::new(&path)
        .oneshot(req)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .into_response();

    let h = resp.headers_mut();
    // 공유된 HTML 이 이 서버의 출처에서 스크립트를 실행하지 못하게 격리한다
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if q.contains_key("dl") {
        let enc = utf8_percent_encode(&name, NON_ALPHANUMERIC);
        if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{enc}")) {
            h.insert(header::CONTENT_DISPOSITION, v);
        }
    }
    Ok(resp)
}

async fn upload(
    State(state): State<Arc<AppState>>,
    Path(p): Path<HashMap<String, String>>,
    mut mp: Multipart,
) -> Result<StatusCode, ApiError> {
    if !state.allow_upload {
        return Err((StatusCode::FORBIDDEN, "업로드가 허용되지 않았습니다 (--upload 로 실행)".to_string()));
    }
    let (share, rel) = params(&p);
    let dir = resolve(&state, share, rel)?;
    if !dir.is_dir() {
        return Err((StatusCode::BAD_REQUEST, "폴더가 아닙니다".to_string()));
    }

    let bad = |e: &dyn std::fmt::Display| (StatusCode::BAD_REQUEST, format!("업로드 실패: {e}"));
    while let Some(mut field) = mp.next_field().await.map_err(|e| bad(&e))? {
        let Some(fname) = field.file_name().map(str::to_owned) else {
            continue;
        };
        // 경로 성분을 떼어 내고 파일 이름만 쓴다
        let fname = FsPath::new(&fname)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.contains(':'))
            .ok_or((StatusCode::BAD_REQUEST, "잘못된 파일 이름입니다".to_string()))?;

        let target = dir.join(&fname);
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    (StatusCode::CONFLICT, format!("같은 이름의 파일이 이미 있습니다: {fname}"))
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("파일을 만들지 못했습니다: {e}"))
                }
            })?;

        let result: Result<(), ApiError> = async {
            while let Some(chunk) = field.chunk().await.map_err(|e| bad(&e))? {
                f.write_all(&chunk)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("쓰기 실패: {e}")))?;
            }
            Ok(())
        }
        .await;
        drop(f);
        if let Err(e) = result {
            // 중간에 끊긴 불완전한 파일은 남기지 않는다
            let _ = tokio::fs::remove_file(&target).await;
            return Err(e);
        }
    }
    Ok(StatusCode::CREATED)
}
