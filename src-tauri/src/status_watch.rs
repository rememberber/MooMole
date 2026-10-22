//! Latest-only status stream. A read lease prevents orphaned WebViews from
//! leaving an indefinite collector; stopping never restarts the collector.
use crate::{
    db,
    engine::{self, Job},
    process,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const LEASE: Duration = Duration::from_secs(12);
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Interval {
    One,
    Two,
    Five,
}
impl TryFrom<u64> for Interval {
    type Error = String;
    fn try_from(seconds: u64) -> Result<Self, String> {
        match seconds {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            5 => Ok(Self::Five),
            _ => Err("采样周期仅支持 1、2 或 5 秒，请重新选择。".into()),
        }
    }
}
impl Interval {
    fn argument(self) -> &'static str {
        match self {
            Self::One => "1s",
            Self::Two => "2s",
            Self::Five => "5s",
        }
    }
    fn seconds(self) -> u64 {
        match self {
            Self::One => 1,
            Self::Two => 2,
            Self::Five => 5,
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    version: u8,
    pub id: String,
    pub state: String,
    interval_seconds: u64,
    sequence: u64,
    received_at: Option<i64>,
    data: Option<Value>,
    error: Option<String>,
}
struct Session {
    snapshot: Snapshot,
    touched: Instant,
}
#[derive(Default)]
pub(crate) struct Store(Option<Session>);
impl Store {
    pub fn begin(&mut self, id: &str, interval: Interval) -> Result<(), String> {
        if self
            .0
            .as_ref()
            .is_some_and(|s| matches!(s.snapshot.state.as_str(), "starting" | "running"))
        {
            return Err("已有状态采集正在进行。".into());
        }
        self.0 = Some(Session {
            snapshot: Snapshot {
                version: 2,
                id: id.into(),
                state: "starting".into(),
                interval_seconds: interval.seconds(),
                sequence: 0,
                received_at: None,
                data: None,
                error: None,
            },
            touched: Instant::now(),
        });
        Ok(())
    }
    pub fn read(&mut self, id: &str) -> Result<Snapshot, String> {
        let s = self
            .0
            .as_mut()
            .filter(|s| s.snapshot.id == id)
            .ok_or("这次状态采集已不可用，请重新开始。")?;
        // A returning WebView must not revive a lease that already expired.
        if s.touched.elapsed() >= LEASE
            && matches!(s.snapshot.state.as_str(), "starting" | "running")
        {
            return Err("页面离开或休眠后采集已停止，请重新开始。".into());
        }
        s.touched = Instant::now();
        Ok(s.snapshot.clone())
    }
    fn alive(&self, id: &str) -> bool {
        self.0
            .as_ref()
            .is_some_and(|s| s.snapshot.id == id && s.touched.elapsed() < LEASE)
    }
    fn receive(&mut self, id: &str, bytes: &[u8]) -> Result<(), String> {
        let value = validate(bytes)?;
        let s = self
            .0
            .as_mut()
            .filter(|s| s.snapshot.id == id)
            .ok_or("状态采集已被替换。")?;
        if !matches!(s.snapshot.state.as_str(), "starting" | "running") {
            return Err("状态采集已结束。".into());
        }
        s.snapshot.sequence += 1;
        s.snapshot.state = "running".into();
        s.snapshot.received_at = Some(db::now());
        s.snapshot.data = Some(value);
        Ok(())
    }
    pub fn finish(&mut self, id: &str, error: Option<String>) {
        if let Some(s) = self.0.as_mut().filter(|s| s.snapshot.id == id) {
            s.snapshot.state = if error.is_some() { "failed" } else { "stopped" }.into();
            s.snapshot.error = error;
        }
    }
}

// Validate fields consumed by the current UI before publishing any snapshot.
// Publish only this supported subset; no unbounded history or event queue.
#[derive(Deserialize, Serialize)]
struct Metrics {
    collected_at: String,
    host: Option<String>,
    platform: Option<String>,
    uptime: Option<String>,
    hardware: Option<Hardware>,
    cpu: Option<Cpu>,
    memory: Option<Memory>,
    gpu: Option<Vec<Gpu>>,
    batteries: Option<Vec<Battery>>,
    thermal: Option<Thermal>,
    disks: Option<Vec<Disk>>,
    disk_io: Option<DiskIo>,
    network: Option<Vec<Network>>,
    top_processes: Option<Vec<Process>>,
    process_collected_at: Option<String>,
    process_stale: Option<bool>,
}
#[derive(Deserialize, Serialize)]
struct Hardware {
    model: Option<String>,
    cpu_model: Option<String>,
    total_ram: Option<String>,
    os_version: Option<String>,
}
#[derive(Deserialize, Serialize)]
struct Cpu {
    usage: Option<f64>,
    logical_cpu: Option<u32>,
    per_core_estimated: Option<bool>,
}
#[derive(Deserialize, Serialize)]
struct Memory {
    total: Option<u64>,
    used: Option<u64>,
    used_percent: Option<f64>,
    pressure: Option<String>,
    available: Option<u64>,
    swap_used: Option<u64>,
    swap_total: Option<u64>,
}
#[derive(Deserialize, Serialize)]
struct Gpu {
    name: Option<String>,
    usage: Option<f64>,
    core_count: Option<i64>,
}
#[derive(Deserialize, Serialize)]
struct Battery {
    percent: Option<f64>,
    status: Option<String>,
    cycle_count: Option<i64>,
    capacity: Option<i64>,
}
#[derive(Deserialize, Serialize)]
struct Thermal {
    battery_temp: Option<f64>,
}
#[derive(Deserialize, Serialize)]
struct Disk {
    mount: String,
    total: u64,
    used: u64,
    used_percent: f64,
    external: bool,
}
#[derive(Deserialize, Serialize)]
struct Process {
    pid: u32,
    name: String,
    cpu: Option<f64>,
    memory_bytes: Option<u64>,
}
#[derive(Deserialize, Serialize)]
struct DiskIo {
    read_rate: Option<f64>,
    write_rate: Option<f64>,
}
#[derive(Deserialize, Serialize)]
struct Network {
    name: String,
    rx_rate_mbs: Option<f64>,
    tx_rate_mbs: Option<f64>,
}
pub(crate) fn validate(bytes: &[u8]) -> Result<Value, String> {
    let m: Metrics =
        serde_json::from_slice(bytes).map_err(|_| "Mole 状态记录格式无法识别，已停止采集。")?;
    let mut texts = vec![
        Some(&m.collected_at),
        m.host.as_ref(),
        m.platform.as_ref(),
        m.uptime.as_ref(),
        m.process_collected_at.as_ref(),
    ];
    if m.collected_at.is_empty() || m.collected_at.len() > 64 {
        return Err("状态记录缺少有效采样时间。".into());
    }
    if let Some(h) = &m.hardware {
        texts.extend([
            h.model.as_ref(),
            h.cpu_model.as_ref(),
            h.total_ram.as_ref(),
            h.os_version.as_ref(),
        ]);
    }
    if let Some(mem) = &m.memory {
        texts.push(mem.pressure.as_ref());
    }
    if let Some(gpus) = &m.gpu {
        if gpus.len() > 8 {
            return Err("状态记录 GPU 数量超过支持范围。".into());
        }
        texts.extend(gpus.iter().map(|gpu| gpu.name.as_ref()));
    }
    if let Some(batteries) = &m.batteries {
        if batteries.len() > 4 {
            return Err("状态记录电池数量超过支持范围。".into());
        }
        texts.extend(batteries.iter().map(|battery| battery.status.as_ref()));
    }
    if let Some(disks) = &m.disks {
        if disks.len() > 3 {
            return Err("状态记录磁盘数量超过上限。".into());
        }
        for d in disks {
            texts.push(Some(&d.mount));
        }
    }
    if let Some(procs) = &m.top_processes {
        if procs.len() > 5 {
            return Err("状态记录进程样本数量超过支持范围。".into());
        }
        for p in procs {
            texts.push(Some(&p.name));
        }
    }
    if let Some(network) = &m.network {
        if network.len() > 3 {
            return Err("状态记录网络接口超过支持范围。".into());
        }
        let mut names = std::collections::HashSet::new();
        for interface in network {
            if interface.name.is_empty() || !names.insert(&interface.name) {
                return Err("状态记录网络接口身份无法识别。".into());
            }
            texts.push(Some(&interface.name));
        }
    }
    if texts.into_iter().flatten().any(|s| s.len() > 4096) {
        return Err("状态记录文本超过上限。".into());
    }
    serde_json::to_value(&m).map_err(|_| "状态记录无法读取。".into())
}

pub(crate) async fn run(
    root: &Path,
    home: &Path,
    id: String,
    job: Arc<Job>,
    store: Arc<Mutex<Store>>,
    interval: Interval,
) -> Result<(), String> {
    let prepared = engine::info(root);
    if !prepared.available {
        return Err(prepared.reason.unwrap_or("引擎不可用。".into()));
    }
    let mut command = engine::command(&root.join("bin/status-go"));
    command
        .args(["--watch", "--interval", interval.argument()])
        .env("HOME", home)
        .current_dir(root);
    let receive_store = store.clone();
    let receive_id = id.clone();
    let stream = process::stream_lines(command, job.clone(), move |bytes| {
        receive_store
            .lock()
            .map_err(|_| "无法更新状态快照。")?
            .receive(&receive_id, bytes)
    });
    tokio::pin!(stream);
    loop {
        tokio::select! {
            result = &mut stream => return result,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if !store.lock().map_err(|_| "无法核对采集页面状态。")?.alive(&id) {
                    job.cancel();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
