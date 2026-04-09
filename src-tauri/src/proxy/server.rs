use crate::proxy::handler::handle_request;
use crate::state::{AppState, ProxyStatus};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::net::TcpListener;
use tokio::sync::watch;

fn emit_proxy_status(app: &AppHandle) {
    let _ = app.emit("proxy-status-changed", ());
}

pub async fn run_proxy(
    state: Arc<AppState>,
    mut shutdown_rx: watch::Receiver<bool>,
    app: AppHandle,
) {
    let port = state.config.read().await.listen.port;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[proxy] Failed to bind port {}: {}", port, e);
            *state.proxy_status.write().await = ProxyStatus::Error(format!("Port {} busy: {}", port, e));
            emit_proxy_status(&app);
            return;
        }
    };

    println!("[proxy] Listening on http://127.0.0.1:{}", port);
    *state.proxy_status.write().await = ProxyStatus::Running;
    emit_proxy_status(&app);

    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, _)) => {
                        let _ = stream.set_nodelay(true);
                        let state = state.clone();
                        let io = TokioIo::new(stream);
                        tokio::spawn(async move {
                            let service = service_fn(move |req| {
                                let state = state.clone();
                                async move { handle_request(req, state).await }
                            });
                            if let Err(e) = http1::Builder::new()
                                .serve_connection(io, service)
                                .await
                            {
                                eprintln!("[proxy] Connection error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("[proxy] Accept error: {}", e);
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    println!("[proxy] Shutting down");
                    *state.proxy_status.write().await = ProxyStatus::Stopped;
                    emit_proxy_status(&app);
                    return;
                }
            }
        }
    }
}
