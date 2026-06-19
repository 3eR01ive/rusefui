//! FFT-спектрограмма knock scope на хосте (как `software_knock.cpp` + `fft.hpp`).

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine};
use rustfft::num_complex::Complex;
use rustfft::Fft;
use serde::Serialize;

pub const KNOCK_ADC_SAMPLE_RATE_HZ: f32 = 218_750.0;

pub const FFT_SIZE: usize = 1024;
/// Верхняя частота heatmap (Гц); отображение и шкала — на стороне UI.
pub const SPECTROGRAM_MAX_FREQ_HZ: f32 = 20_000.0;
const ADC_RATIO: f32 = 3.3 / 4095.0;
const SENSITIVITY: f32 = 1.0;
const DBFS_MIN: f32 = -100.0;
const DBFS_MAX: f32 = -20.0;
/// Опорная амплитуда FFT для u8 → dBFS; поднята, чтобы типичный knock не клиповал в 255.
const DBFS_REF_AMPLITUDE: f32 = 8.0;
/// Не учитывать DC и самую низкую полосу при поиске пика (шум/утечка после AC).
const MIN_PEAK_BIN: usize = 2;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnockSpectrogramView {
    pub width: usize,
    pub height: usize,
    /// column-major: index = time_col * height + freq_bin (bin 0 = DC)
    pub pixels: Vec<u8>,
}

/// Инкремент для UI: новые FFT-столбцы + сдвиг скользящего окна (без полной heatmap).
#[derive(Debug, Clone, Copy, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnockSpectrogramMarker {
    /// Индекс FFT-столбца в текущем скользящем окне (0 = левый край heatmap).
    pub column: usize,
    pub cylinder: u8,
    pub channel: u8,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnockSpectrogramPatch {
    pub width: usize,
    pub height: usize,
    /// Сколько столбцов убрано слева (deque на max_columns).
    pub shift_left: usize,
    /// column-major новые столбцы: `new_column_count * height` байт.
    pub new_columns: Vec<u8>,
    /// Вертикальные метки смены цилиндра для новых столбцов в этом patch.
    pub new_markers: Vec<KnockSpectrogramMarker>,
}

/// Заголовок GPU-пакета (LE): width, height — по 4 байта; bytes 8–15 зарезервированы (0).
pub const KNOCK_SPECTROGRAM_GPU_HEADER: usize = 16;

/// Row-major u8 heatmap + заголовок для WebGL (`texImage2D`).
pub fn encode_knock_spectrogram_gpu(view: &KnockSpectrogramView) -> Vec<u8> {
    let w = view.width;
    let h = view.height;
    let mut buf = Vec::with_capacity(KNOCK_SPECTROGRAM_GPU_HEADER + w * h);
    buf.extend_from_slice(&(w as u32).to_le_bytes());
    buf.extend_from_slice(&(h as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    if w == 0 || h == 0 {
        return buf;
    }
    for row in 0..h {
        for col in 0..w {
            let idx = col * h + row;
            buf.push(view.pixels.get(idx).copied().unwrap_or(0));
        }
    }
    buf
}

pub fn encode_knock_spectrogram_gpu_b64(view: &KnockSpectrogramView) -> String {
    STANDARD.encode(encode_knock_spectrogram_gpu(view))
}

/// Patch-пакет (24 байта + column-major новые столбцы) для инкрементального WebGL.
pub const KNOCK_SPECTROGRAM_GPU_PATCH_HEADER: usize = 24;

pub fn encode_knock_spectrogram_gpu_patch(patch: &KnockSpectrogramPatch) -> Vec<u8> {
    let w = patch.width;
    let h = patch.height.max(1);
    let new_col_count = patch.new_columns.len() / h;
    let mut buf =
        Vec::with_capacity(KNOCK_SPECTROGRAM_GPU_PATCH_HEADER + patch.new_columns.len());
    buf.extend_from_slice(&(w as u32).to_le_bytes());
    buf.extend_from_slice(&(patch.height as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(patch.shift_left as u32).to_le_bytes());
    buf.extend_from_slice(&(new_col_count as u32).to_le_bytes());
    buf.extend_from_slice(&patch.new_columns);
    buf
}

pub fn encode_knock_spectrogram_gpu_patch_b64(patch: &KnockSpectrogramPatch) -> String {
    STANDARD.encode(encode_knock_spectrogram_gpu_patch(patch))
}

pub fn spectrogram_height_bins(sample_rate_hz: f32) -> usize {
    let max_bin =
        ((SPECTROGRAM_MAX_FREQ_HZ * FFT_SIZE as f32) / sample_rate_hz).floor() as usize;
    max_bin.min(FFT_SIZE / 2 - 1) + 1
}

pub fn bin_to_frequency_hz(bin: usize, sample_rate_hz: f32) -> f32 {
    bin as f32 * sample_rate_hz / FFT_SIZE as f32
}

pub struct KnockSpectrogramEngine {
    sample_rate_hz: f32,
    height_bins: usize,
    /// Ширина viewport (столбцов FFT) из `window_ms`.
    view_columns_max: usize,
    /// Левый край viewport в полной записи (глобальный индекс столбца).
    view_start: usize,
    /// Пока true — viewport прижат к хвосту записи.
    follow_live: bool,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    /// Полная запись прогона (столбец = один knock-захват).
    columns: VecDeque<Vec<u8>>,
    /// Метки цилиндров; `column` — глобальный индекс в `columns`.
    markers: Vec<KnockSpectrogramMarker>,
    last_emit_view_start: usize,
    last_emit_total_columns: usize,
    /// Максимум u8 за весь прогон (не только видимое окно).
    run_peak_val: u8,
    run_peak_bin: usize,
}

impl KnockSpectrogramEngine {
    pub fn new(sample_rate_hz: f32, window_ms: u32) -> Self {
        let view_columns_max = max_columns_for_window(window_ms, sample_rate_hz);
        let height_bins = spectrogram_height_bins(sample_rate_hz);
        let mut planner = rustfft::FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window = blackman_harris(FFT_SIZE, true);

        Self {
            sample_rate_hz,
            height_bins,
            view_columns_max,
            view_start: 0,
            follow_live: true,
            fft,
            window,
            scratch: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            columns: VecDeque::new(),
            markers: Vec::new(),
            last_emit_view_start: 0,
            last_emit_total_columns: 0,
            run_peak_val: 0,
            run_peak_bin: MIN_PEAK_BIN,
        }
    }

    pub fn clear(&mut self) {
        self.columns.clear();
        self.markers.clear();
        self.view_start = 0;
        self.follow_live = true;
        self.last_emit_view_start = 0;
        self.last_emit_total_columns = 0;
        self.run_peak_val = 0;
        self.run_peak_bin = MIN_PEAK_BIN;
    }

    pub fn set_view_columns_max(&mut self, window_ms: u32) {
        self.view_columns_max = max_columns_for_window(window_ms, self.sample_rate_hz);
        if self.follow_live {
            self.view_start = self
                .columns
                .len()
                .saturating_sub(self.view_columns_max);
        }
        self.view_start = self.clamp_view_start();
    }

    /// Задать ширину viewport напрямую в событиях (FFT-столбцах), минуя ms-конвертацию.
    pub fn set_view_columns_events(&mut self, n: usize) {
        self.view_columns_max = n.max(1);
        if self.follow_live {
            self.view_start = self
                .columns
                .len()
                .saturating_sub(self.view_columns_max);
        }
        self.view_start = self.clamp_view_start();
    }

    /// Сохранить всю запись в бинарный лог-файл.
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        let h = self.height_bins;
        let total = self.columns.len() as u32;
        let marker_count = self.markers.len() as u32;
        let mut file = std::fs::File::create(path)
            .map_err(|e| format!("create knock log: {e}"))?;
        // Header: magic(8) version(4) sample_rate(4) height(4) total_cols(4) markers(4)
        file.write_all(b"RUSFKSP1").map_err(|e| e.to_string())?;
        file.write_all(&1u32.to_le_bytes()).map_err(|e| e.to_string())?;
        file.write_all(&self.sample_rate_hz.to_le_bytes()).map_err(|e| e.to_string())?;
        file.write_all(&(h as u32).to_le_bytes()).map_err(|e| e.to_string())?;
        file.write_all(&total.to_le_bytes()).map_err(|e| e.to_string())?;
        file.write_all(&marker_count.to_le_bytes()).map_err(|e| e.to_string())?;
        // Markers: column(4) cylinder(1) channel(1) pad(2)
        for m in &self.markers {
            file.write_all(&(m.column as u32).to_le_bytes()).map_err(|e| e.to_string())?;
            file.write_all(&[m.cylinder, m.channel, 0, 0]).map_err(|e| e.to_string())?;
        }
        // Pixel columns (column-major, h bytes each)
        let pad = vec![0u8; h];
        for col in &self.columns {
            let to_write = if col.len() >= h { &col[..h] } else { col.as_slice() };
            file.write_all(to_write).map_err(|e| e.to_string())?;
            if to_write.len() < h {
                file.write_all(&pad[..h - to_write.len()]).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Загрузить запись из бинарного лог-файла (формат `save_to_file`).
    pub fn load_from_file(path: &std::path::Path, window_ms: u32) -> Result<Self, String> {
        use std::io::Read;
        let mut file =
            std::fs::File::open(path).map_err(|e| format!("open knock log: {e}"))?;
        // Header: magic(8) version(4) sample_rate(4) height(4) total_cols(4) markers(4)
        let mut header = [0u8; 28];
        file.read_exact(&mut header)
            .map_err(|e| format!("read knock log header: {e}"))?;
        if &header[0..8] != b"RUSFKSP1" {
            return Err("неизвестный формат knock log".into());
        }
        let version = u32::from_le_bytes(header[8..12].try_into().unwrap());
        if version != 1 {
            return Err(format!("неподдерживаемая версия knock log: {version}"));
        }
        let sample_rate_hz = f32::from_le_bytes(header[12..16].try_into().unwrap());
        let saved_height = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        let total = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
        let marker_count = u32::from_le_bytes(header[24..28].try_into().unwrap()) as usize;

        let mut markers = Vec::with_capacity(marker_count);
        for _ in 0..marker_count {
            let mut mbuf = [0u8; 8];
            file.read_exact(&mut mbuf)
                .map_err(|e| format!("read knock log marker: {e}"))?;
            markers.push(KnockSpectrogramMarker {
                column: u32::from_le_bytes(mbuf[0..4].try_into().unwrap()) as usize,
                cylinder: mbuf[4],
                channel: mbuf[5],
            });
        }

        let sr = if sample_rate_hz.is_finite() && sample_rate_hz > 0.0 {
            sample_rate_hz
        } else {
            KNOCK_ADC_SAMPLE_RATE_HZ
        };
        let mut eng = Self::new(sr, window_ms);
        let h = eng.height_bins;
        let mut columns: VecDeque<Vec<u8>> = VecDeque::with_capacity(total);
        for _ in 0..total {
            let mut col = vec![0u8; saved_height];
            file.read_exact(&mut col)
                .map_err(|e| format!("read knock log column: {e}"))?;
            // Привести к расчётной высоте текущего движка (обычно совпадает).
            if saved_height != h {
                col.resize(h, 0);
            }
            columns.push_back(col);
        }

        eng.load_columns(columns, markers);
        Ok(eng)
    }

    /// Заменить запись загруженным логом (для просмотра): пересчитать пик и прижать viewport к началу.
    pub fn load_columns(
        &mut self,
        columns: VecDeque<Vec<u8>>,
        markers: Vec<KnockSpectrogramMarker>,
    ) {
        self.clear();
        for col in &columns {
            self.note_column_peak(col);
        }
        self.columns = columns;
        self.markers = markers;
        self.follow_live = false;
        self.view_start = 0;
        self.view_start = self.clamp_view_start();
    }

    pub fn set_follow_live(&mut self, follow: bool) {
        self.follow_live = follow;
        if follow {
            self.view_start = self
                .columns
                .len()
                .saturating_sub(self.view_columns_max);
        }
        self.view_start = self.clamp_view_start();
    }

    pub fn pan_view(&mut self, delta_columns: i32) {
        self.follow_live = false;
        let vw = self.viewport_width().max(1);
        let max_start = self.columns.len().saturating_sub(vw);
        let next = (self.view_start as i64 + i64::from(delta_columns)).clamp(0, max_start as i64);
        self.view_start = next as usize;
    }

    pub fn total_columns(&self) -> usize {
        self.columns.len()
    }

    pub fn viewport_stats(&self) -> (usize, usize, usize, bool) {
        (
            self.columns.len(),
            self.clamp_view_start(),
            self.viewport_width(),
            self.follow_live,
        )
    }

    /// Забрать diff viewport для WebGL (не копирует всю запись).
    pub fn take_ui_patch(&mut self) -> KnockSpectrogramPatch {
        let height = self.height_bins;
        let vs = self.clamp_view_start();
        self.view_start = vs;
        let vw = self.viewport_width();
        let total = self.columns.len();

        if total == 0 || vw == 0 {
            self.last_emit_view_start = vs;
            self.last_emit_total_columns = 0;
            return KnockSpectrogramPatch {
                width: 0,
                height,
                ..Default::default()
            };
        }

        let view_changed = vs != self.last_emit_view_start;
        let added = total.saturating_sub(self.last_emit_total_columns);

        if !view_changed && added == 0 {
            return KnockSpectrogramPatch {
                width: vw,
                height,
                ..Default::default()
            };
        }

        let (shift_left, new_columns, new_markers) = if view_changed {
            self.build_full_viewport_emit(vs, vw, height)
        } else if vw < self.view_columns_max {
            self.build_append_emit(vs, vw, height, added)
        } else {
            self.build_scroll_emit(vs, vw, height, added)
        };

        self.last_emit_view_start = vs;
        self.last_emit_total_columns = total;

        KnockSpectrogramPatch {
            width: vw,
            height,
            shift_left,
            new_columns,
            new_markers,
        }
    }

    pub fn push_samples(&mut self, samples: &[f32]) {
        self.push_samples_with_marker(samples, 0, 0);
    }

    /// Одно ECU-окно → один FFT-столбец; метка на этом столбце (без склейки с другими цилиндрами).
    pub fn push_samples_with_marker(&mut self, samples: &[f32], cylinder: u8, channel: u8) {
        let Some(col) = self.fft_one_column(samples) else {
            return;
        };
        let global_col = self.columns.len();
        self.append_column(col);
        self.markers.push(KnockSpectrogramMarker {
            column: global_col,
            cylinder,
            channel,
        });
    }

    pub fn visible_markers(&self) -> Vec<KnockSpectrogramMarker> {
        let vs = self.clamp_view_start();
        let vw = self.viewport_width();
        if vw == 0 {
            return Vec::new();
        }
        let ve = vs + vw;
        self.markers
            .iter()
            .filter(|m| m.column >= vs && m.column < ve)
            .map(|m| KnockSpectrogramMarker {
                column: m.column - vs,
                cylinder: m.cylinder,
                channel: m.channel,
            })
            .collect()
    }

    pub fn spectrogram_meta(&self) -> (usize, usize) {
        (self.viewport_width(), self.height_bins)
    }

    /// Пик за весь прогон (максимальная амплитуда среди всех FFT-столбцов, включая ушедшие из окна).
    pub fn peak_frequency_hz(&self) -> Option<f32> {
        if self.run_peak_val == 0 {
            return None;
        }
        Some(bin_to_frequency_hz(self.run_peak_bin, self.sample_rate_hz))
    }

    fn note_column_peak(&mut self, col: &[u8]) {
        for (row, &v) in col.iter().enumerate().skip(MIN_PEAK_BIN) {
            if v > self.run_peak_val {
                self.run_peak_val = v;
                self.run_peak_bin = row;
            }
        }
    }

    /// Viewport для GPU / snapshot (не вся запись).
    pub fn view(&self) -> KnockSpectrogramView {
        let height = self.height_bins;
        let vs = self.clamp_view_start();
        let vw = self.viewport_width();
        if vw == 0 {
            return KnockSpectrogramView {
                width: 0,
                height,
                pixels: Vec::new(),
            };
        }

        let mut pixels = Vec::with_capacity(vw * height);
        for col_idx in vs..vs + vw {
            if let Some(col) = self.columns.get(col_idx) {
                pixels.extend_from_slice(col);
            }
        }

        KnockSpectrogramView {
            width: vw,
            height,
            pixels,
        }
    }

    /// FFT по одному knock-окну: до `FFT_SIZE` первых сэмплов, остаток — дополнение средним (как DC-steady).
    fn fft_one_column(&mut self, samples: &[f32]) -> Option<Vec<u8>> {
        if samples.is_empty() {
            return None;
        }
        let used = samples.len().min(FFT_SIZE);
        let mean = samples[..used].iter().sum::<f32>() / used as f32;
        for i in 0..FFT_SIZE {
            let adc = if i < samples.len() {
                samples[i]
            } else {
                mean
            };
            let voltage = ADC_RATIO * (adc - mean);
            self.scratch[i] = Complex::new(SENSITIVITY * voltage * self.window[i], 0.0);
        }
        for i in FFT_SIZE..self.scratch.len() {
            self.scratch[i] = Complex::new(0.0, 0.0);
        }

        self.fft.process(&mut self.scratch);
        Some(spectrum_column(&self.scratch, self.height_bins))
    }

    fn append_column(&mut self, col: Vec<u8>) {
        self.note_column_peak(&col);
        self.columns.push_back(col);
        if self.follow_live {
            self.view_start = self
                .columns
                .len()
                .saturating_sub(self.view_columns_max);
        }
        self.view_start = self.clamp_view_start();
    }

    fn viewport_width(&self) -> usize {
        self.view_columns_max.min(self.columns.len())
    }

    fn clamp_view_start(&self) -> usize {
        let total = self.columns.len();
        let vw = self.viewport_width();
        if total <= vw {
            return 0;
        }
        self.view_start.min(total - vw)
    }

    fn build_full_viewport_emit(
        &self,
        vs: usize,
        vw: usize,
        height: usize,
    ) -> (usize, Vec<u8>, Vec<KnockSpectrogramMarker>) {
        let mut new_columns = Vec::with_capacity(vw * height);
        for col_idx in vs..vs + vw {
            if let Some(col) = self.columns.get(col_idx) {
                new_columns.extend_from_slice(col);
            }
        }
        (
            vw,
            new_columns,
            self.markers_for_global_range(vs, vs + vw, vs),
        )
    }

    fn build_append_emit(
        &self,
        vs: usize,
        vw: usize,
        height: usize,
        added: usize,
    ) -> (usize, Vec<u8>, Vec<KnockSpectrogramMarker>) {
        let old_vw = vw.saturating_sub(added);
        let mut new_columns = Vec::with_capacity(added * height);
        for col_idx in vs + old_vw..vs + vw {
            if let Some(col) = self.columns.get(col_idx) {
                new_columns.extend_from_slice(col);
            }
        }
        (
            0,
            new_columns,
            self.markers_for_global_range(vs + old_vw, vs + vw, vs),
        )
    }

    fn build_scroll_emit(
        &self,
        vs: usize,
        vw: usize,
        height: usize,
        added: usize,
    ) -> (usize, Vec<u8>, Vec<KnockSpectrogramMarker>) {
        let shift = added.min(vw);
        let start = vs + vw - shift;
        let mut new_columns = Vec::with_capacity(shift * height);
        for col_idx in start..vs + vw {
            if let Some(col) = self.columns.get(col_idx) {
                new_columns.extend_from_slice(col);
            }
        }
        (
            shift,
            new_columns,
            self.markers_for_global_range(start, vs + vw, vs),
        )
    }

    fn markers_for_global_range(
        &self,
        global_start: usize,
        global_end: usize,
        viewport_start: usize,
    ) -> Vec<KnockSpectrogramMarker> {
        self.markers
            .iter()
            .filter(|m| m.column >= global_start && m.column < global_end)
            .map(|m| KnockSpectrogramMarker {
                column: m.column - viewport_start,
                cylinder: m.cylinder,
                channel: m.channel,
            })
            .collect()
    }
}

pub fn max_columns_for_window(window_ms: u32, _sample_rate_hz: f32) -> usize {
    // Один столбец на knock-окно; запас под 12 цил. @ 12k rpm 4-такт.
    const MAX_CAPTURES_PER_SEC: f64 = 1200.0;
    let win_sec = f64::from(window_ms) / 1000.0;
    ((win_sec * MAX_CAPTURES_PER_SEC).ceil() as usize)
        .max(64)
        .saturating_add(8)
}

fn spectrum_column(scratch: &[Complex<f32>], bin_count: usize) -> Vec<u8> {
    let mut col = Vec::with_capacity(bin_count);
    for bin in 0..bin_count {
        if bin == 0 {
            col.push(0);
            continue;
        }
        let c = &scratch[bin];
        let amp = (c.re * c.re + c.im * c.im).sqrt();
        col.push(amplitude_to_dbfs_u8(amp));
    }
    col
}

/// Частота пика по heatmap (максимальная амплитуда среди всех колонок).
pub fn peak_frequency_hz(view: &KnockSpectrogramView, sample_rate_hz: f32) -> Option<f32> {
    if view.width == 0 || view.height == 0 || view.pixels.is_empty() {
        return None;
    }
    let mut best_val = 0u8;
    let mut best_row = MIN_PEAK_BIN;
    for col in 0..view.width {
        for row in MIN_PEAK_BIN..view.height {
            let idx = col * view.height + row;
            let v = *view.pixels.get(idx)?;
            if v > best_val {
                best_val = v;
                best_row = row;
            }
        }
    }
    if best_val == 0 {
        return None;
    }
    Some(bin_to_frequency_hz(best_row, sample_rate_hz))
}

fn amplitude_to_dbfs_u8(amplitude: f32) -> u8 {
    let dbfs = 20.0 * (amplitude.max(1e-20) / DBFS_REF_AMPLITUDE).log10();
    dbfs_to_u8(dbfs)
}

fn dbfs_to_u8(dbfs: f32) -> u8 {
    let t = (dbfs - DBFS_MIN) / (DBFS_MAX - DBFS_MIN);
    (t.clamp(0.0, 1.0) * 255.0) as u8
}

/// Blackman–Harris (как `fft::blackmanharris`, `sflag=true`).
fn blackman_harris(n: usize, periodic: bool) -> Vec<f32> {
    let coeff = [0.35875f32, -0.48829, 0.14128, -0.01168];
    let wlength = if periodic && n > 1 { n - 1 } else { n };
    let mut w = vec![0.0f32; n];
    for i in 0..n {
        let mut wi = 0.0f32;
        for (j, &c) in coeff.iter().enumerate() {
            wi += c * (i as f32 * j as f32 * 2.0 * PI / wlength as f32).cos();
        }
        w[i] = wi;
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_bins_covers_zero_to_20khz() {
        let h = spectrogram_height_bins(KNOCK_ADC_SAMPLE_RATE_HZ);
        assert!(h > 32);
        let top_hz = bin_to_frequency_hz(h - 1, KNOCK_ADC_SAMPLE_RATE_HZ);
        assert!(top_hz <= SPECTROGRAM_MAX_FREQ_HZ);
        assert!(top_hz > 19_000.0);
    }

    #[test]
    fn engine_produces_one_column_per_capture() {
        let mut eng = KnockSpectrogramEngine::new(KNOCK_ADC_SAMPLE_RATE_HZ, 500);
        let sr = KNOCK_ADC_SAMPLE_RATE_HZ;
        let mut buf = Vec::new();
        for i in 0..FFT_SIZE {
            let t = i as f32 / sr;
            let v = (2.0 * PI * 8000.0 * t).sin() * 2000.0 + 2000.0;
            buf.push(v);
        }
        eng.push_samples(&buf);
        let view = eng.view();
        assert_eq!(view.width, 1);
        assert_eq!(view.pixels.len(), view.width * view.height);
    }

    #[test]
    fn each_capture_adds_one_column_and_marker() {
        let mut eng = KnockSpectrogramEngine::new(KNOCK_ADC_SAMPLE_RATE_HZ, 500);
        let short = vec![1500.0; 400];
        eng.push_samples_with_marker(&short, 0, 0);
        eng.push_samples_with_marker(&short, 1, 0);
        assert_eq!(eng.view().width, 2);
        assert_eq!(eng.visible_markers().len(), 2);
        assert_eq!(eng.visible_markers()[0].cylinder, 0);
        assert_eq!(eng.visible_markers()[1].cylinder, 1);
    }

    #[test]
    fn short_window_still_one_column() {
        let mut eng = KnockSpectrogramEngine::new(KNOCK_ADC_SAMPLE_RATE_HZ, 500);
        eng.push_samples_with_marker(&vec![1000.0; 200], 2, 0);
        assert_eq!(eng.view().width, 1);
    }

    #[test]
    fn gpu_packet_row_major() {
        let view = KnockSpectrogramView {
            width: 2,
            height: 2,
            pixels: vec![1, 2, 3, 4],
        };
        let buf = encode_knock_spectrogram_gpu(&view);
        assert_eq!(buf.len(), 16 + 4);
        assert_eq!(&buf[16..], &[1, 3, 2, 4]);
    }

    #[test]
    fn run_peak_is_max_over_entire_run_not_viewport() {
        let h = spectrogram_height_bins(KNOCK_ADC_SAMPLE_RATE_HZ);
        let mut eng = KnockSpectrogramEngine::new(KNOCK_ADC_SAMPLE_RATE_HZ, 40);

        let mut early = vec![0u8; h];
        early[30] = 120;
        let early_hz = bin_to_frequency_hz(30, KNOCK_ADC_SAMPLE_RATE_HZ);

        let mut later = vec![0u8; h];
        later[55] = 200;
        let run_peak_hz = bin_to_frequency_hz(55, KNOCK_ADC_SAMPLE_RATE_HZ);

        eng.feed_test_column(early);
        assert_eq!(eng.peak_frequency_hz(), Some(early_hz));

        eng.feed_test_column(later);
        assert_eq!(eng.peak_frequency_hz(), Some(run_peak_hz));

        for _ in 0..80 {
            let mut quiet = vec![0u8; h];
            quiet[3] = 10;
            eng.feed_test_column(quiet);
        }

        assert_eq!(eng.total_columns(), 82);
        assert!(eng.view().width <= eng.total_columns());
        assert_eq!(
            eng.peak_frequency_hz(),
            Some(run_peak_hz),
            "пик прогона не должен сбрасываться при прокрутке viewport"
        );
    }

    #[test]
    fn pan_view_shows_earlier_captures() {
        let mut eng = KnockSpectrogramEngine::new(KNOCK_ADC_SAMPLE_RATE_HZ, 500);
        eng.set_viewport_columns_max_raw(3);
        let short = vec![1500.0; 400];
        for cyl in 0..6u8 {
            eng.push_samples_with_marker(&short, cyl, 0);
        }
        assert_eq!(eng.total_columns(), 6);
        assert_eq!(eng.view().width, 3);
        eng.set_follow_live(false);
        eng.pan_view(-3);
        assert_eq!(eng.viewport_stats().1, 0);
        let markers = eng.visible_markers();
        assert_eq!(markers.len(), 3);
        assert_eq!(markers[0].cylinder, 0);
    }
}

#[cfg(test)]
impl KnockSpectrogramEngine {
    fn feed_test_column(&mut self, col: Vec<u8>) {
        self.append_column(col);
    }

    fn set_viewport_columns_max_raw(&mut self, width: usize) {
        self.view_columns_max = width.max(1);
        if self.follow_live {
            self.view_start = self
                .columns
                .len()
                .saturating_sub(self.view_columns_max);
        }
        self.view_start = self.clamp_view_start();
    }
}
