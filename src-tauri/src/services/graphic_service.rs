use crate::{log_debug, log_error, log_info, log_internal, log_warn};
use nvapi;
use nvapi::UtilizationDomain;
use tokio::task::spawn_blocking;
use tokio::task::JoinError;

///
/// GPU使用率を取得する（NVAPI を使用）
///
pub async fn get_nvidia_gpu_usage() -> Result<f32, nvapi::Status> {
  let handle = spawn_blocking(|| {
    log_debug!("start", "get_nvidia_gpu_usage", None::<&str>);

    let gpus = nvapi::PhysicalGpu::enumerate()?;

    if gpus.is_empty() {
      log_warn!(
        "not found",
        "get_nvidia_gpu_usage",
        Some("gpu is not found")
      );
      tracing::warn!("gpu is not found");
      return Err(nvapi::Status::Error); // GPUが見つからない場合はエラーを返す
    }

    let mut total_usage = 0.0;
    let mut gpu_count = 0;

    for gpu in gpus.iter() {
      let usage = match gpu.usages() {
        Ok(usage) => usage,
        Err(e) => {
          log_error!("usages_failed", "get_nvidia_gpu_usage", Some(e.to_string()));
          return Err(e);
        }
      };

      if let Some(gpu_usage) = usage.get(&UtilizationDomain::Graphics) {
        let usage_f32 = gpu_usage.0 as f32 / 100.0; // Percentage を f32 に変換
        total_usage += usage_f32;
        gpu_count += 1;
      }
    }

    log_info!(
      &format!("gpu_count: {:?}", gpu_count),
      "get_nvidia_gpu_usage",
      None::<&str>
    );

    if gpu_count == 0 {
      log_warn!(
        "no_usage",
        "get_nvidia_gpu_usage",
        Some("No GPU usage data collected")
      );
      return Err(nvapi::Status::Error); // 使用率が取得できなかった場合のエラーハンドリング
    }

    let average_usage = total_usage / gpu_count as f32;

    log_debug!("end", "get_nvidia_gpu_usage", None::<&str>);

    Ok(average_usage)
  });

  handle.await.map_err(|e: JoinError| {
    log_error!("join_error", "get_nvidia_gpu_usage", Some(e.to_string()));
    nvapi::Status::Error
  })?
}
