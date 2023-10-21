use std::sync::Arc;
use std::{net::SocketAddr, env};

use axum::Json;
use axum::extract::State;
use axum::{Router, routing::get};
use tokio::{sync::Mutex, time::{Duration, sleep}};
use serde::Serialize;
use serde_json::from_str;
use systemstat::{saturating_sub_bytes, Platform, System};

#[derive(Debug, Clone, Serialize)]
struct AppState {
    cpu_usage: CPU,
    memory_usage: Memory,
    swap_usage: Memory,
    last_updated: i64,
}

#[derive(Debug, Clone, Serialize)]
struct Memory {
    used: u64,
    total: u64,
}

#[derive(Debug, Clone, Serialize)]
struct CPU {
    user: f32,
    nice: f32,
    interrupt: f32,
    system: f32,
    idle: f32,
}

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_module("stat_monitor", {
            if cfg!(debug_assertions) {
                log::LevelFilter::Trace
            } else {
                log::LevelFilter::Info
            }
        })
        .init();

    let shared_state = Arc::new(Mutex::new(AppState {
        cpu_usage: CPU {
            user: 0.0,
            nice: 0.0,
            interrupt: 0.0,
            system: 0.0,
            idle: 0.0,
        },
        memory_usage: Memory {
            used: 0,
            total: 0,
        },
        swap_usage: Memory {
            used: 0,
            total: 0,
        },
        last_updated: 0,
    }));

    let app = Router::new()
        .route("/", get(root))
        .with_state(shared_state);

    let addr = SocketAddr::from((
        [0, 0, 0, 0],
        from_str::<u16>(
            env::var("PORT")
                .unwrap_or("8080".to_string())
                .as_str(),
        )
        .unwrap(),
    ));
    log::info!("Listening on {}", addr);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn root(State(state): State<Arc<Mutex<AppState>>>) -> Json<serde_json::Value> {
    let mut state = state.lock().await;
    let now = chrono::Utc::now().timestamp();
    if now - state.last_updated > 5 {
        let sys = System::new();
        let mem = match sys.memory() {
            Ok(mem) => Some(Memory {
                used: saturating_sub_bytes(mem.total, mem.free).as_u64(),
                total: mem.total.as_u64(),
            }),
            Err(_) => None
        };
        let swap = match sys.swap() {
            Ok(swap) => Some(Memory {
                used: saturating_sub_bytes(swap.total, swap.free).as_u64(),
                total: swap.total.as_u64(),
            }),
            Err(_) => None
        };
        let cpu = sys.cpu_load_aggregate();
        sleep(Duration::from_secs(1)).await;
        let cpu_usage = cpu.and_then(|f| f.done());

        if mem.is_none() {
            return serde_json::json!({ "error": "failed to get memory usage" }).into();
        }
        if swap.is_none() {
            return serde_json::json!({ "error": "failed to get swap usage" }).into();
        }
        if cpu_usage.is_err() {
            return serde_json::json!({ "error": "failed to get cpu usage" }).into();
        }

        let cpu_usage = cpu_usage.unwrap();
        let swap_usage = swap.unwrap();
        let mem_usage = mem.unwrap();

        state.cpu_usage = CPU {
            user: cpu_usage.user * 100.0,
            nice: cpu_usage.nice * 100.0,
            interrupt: cpu_usage.interrupt * 100.0,
            system: cpu_usage.system * 100.0,
            idle: cpu_usage.idle * 100.0,
        };
        state.memory_usage = Memory {
            used: mem_usage.used,
            total: mem_usage.total,
        };
        state.swap_usage = Memory {
            used: swap_usage.used,
            total: swap_usage.total,
        };
        state.last_updated = now;
        return serde_json::json!({
            "cpu": state.cpu_usage,
            "memory": state.memory_usage,
            "swap": state.swap_usage,
        }).into();
    } else {
        return serde_json::json!({
            "cpu": state.cpu_usage,
            "memory": state.memory_usage,
            "swap": state.swap_usage,
        }).into();
    }
}
