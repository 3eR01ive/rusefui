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
pub const HOP: usize = 256;
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
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnockSpectrogramPatch {
    pub width: usize,
    pub height: usize,
    /// Сколько столбцов убрано слева (deque на max_columns).
    pub shift_left: usize,
    /// column-major новые столбцы: `new_column_count * height` байт.
    pub new_columns: Vec<u8>,
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
    max_columns: usize,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    stream: Vec<f32>,
    stream_offset: usize,
    columns: VecDeque<Vec<u8>>,
    pending_new_columns: Vec<u8>,
    popped_since_emit: usize,
    /// Максимум u8 за весь прогон (не только видимое окно).
    run_peak_val: u8,
    run_peak_bin: usize,
}

impl KnockSpectrogramEngine {
    pub fn new(sample_rate_hz: f32, window_ms: u32) -> Self {
        let max_columns = max_columns_for_window(window_ms, sample_rate_hz);
        let height_bins = spectrogram_height_bins(sample_rate_hz);
        let mut planner = rustfft::FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window = blackman_harris(FFT_SIZE, true);

        Self {
            sample_rate_hz,
            height_bins,
            max_columns,
            fft,
            window,
            scratch: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            stream: Vec::new(),
            stream_offset: 0,
            columns: VecDeque::new(),
            pending_new_columns: Vec::new(),
            popped_since_emit: 0,
            run_peak_val: 0,
            run_peak_bin: MIN_PEAK_BIN,
        }
    }

    pub fn clear(&mut self) {
        self.stream.clear();
        self.stream_offset = 0;
        self.columns.clear();
        self.pending_new_columns.clear();
        self.popped_since_emit = 0;
        self.run_peak_val = 0;
        self.run_peak_bin = MIN_PEAK_BIN;
    }

    /// Забрать накопленные столбцы для UI (не копирует всю heatmap).
    pub fn take_ui_patch(&mut self) -> KnockSpectrogramPatch {
        let height = self.height_bins;
        let patch = KnockSpectrogramPatch {
            width: self.columns.len(),
            height,
            shift_left: self.popped_since_emit,
            new_columns: std::mem::take(&mut self.pending_new_columns),
        };
        self.popped_since_emit = 0;
        patch
    }

    pub fn push_samples(&mut self, samples: &[f32]) {
        self.stream.extend_from_slice(samples);
        self.drain_fft_windows();
        self.trim_stream();
    }

    pub fn spectrogram_meta(&self) -> (usize, usize) {
        (self.columns.len(), self.height_bins)
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

    pub fn view(&self) -> KnockSpectrogramView {
        let height = self.height_bins;
        let width = self.columns.len();
        if width == 0 {
            return KnockSpectrogramView {
                width: 0,
                height,
                pixels: Vec::new(),
            };
        }

        let mut pixels = Vec::with_capacity(width * height);
        for col in &self.columns {
            pixels.extend_from_slice(col);
        }

        KnockSpectrogramView {
            width,
            height,
            pixels,
        }
    }

    fn drain_fft_windows(&mut self) {
        while self.stream.len().saturating_sub(self.stream_offset) >= FFT_SIZE {
            let start = self.stream_offset;
            let frame = &self.stream[start..start + FFT_SIZE];

            let mean = frame.iter().sum::<f32>() / frame.len() as f32;
            for (i, &adc) in frame.iter().enumerate() {
                let voltage = ADC_RATIO * (adc - mean);
                self.scratch[i] = Complex::new(SENSITIVITY * voltage * self.window[i], 0.0);
            }
            for i in FFT_SIZE..self.scratch.len() {
                self.scratch[i] = Complex::new(0.0, 0.0);
            }

            self.fft.process(&mut self.scratch);

            let col = spectrum_column(&self.scratch, self.height_bins);
            self.note_column_peak(&col);
            self.pending_new_columns.extend_from_slice(&col);
            self.columns.push_back(col);
            while self.columns.len() > self.max_columns {
                self.columns.pop_front();
                self.popped_since_emit += 1;
            }

            self.stream_offset += HOP;
        }
    }

    fn trim_stream(&mut self) {
        if self.stream_offset > FFT_SIZE * 2 {
            self.stream.drain(..self.stream_offset);
            self.stream_offset = 0;
        }
    }
}

pub fn max_columns_for_window(window_ms: u32, sample_rate_hz: f32) -> usize {
    let samples = (sample_rate_hz as f64 * f64::from(window_ms) / 1000.0).ceil() as usize;
    (samples / HOP).max(32) + 4
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
    fn engine_produces_columns_from_sine() {
        let mut eng = KnockSpectrogramEngine::new(KNOCK_ADC_SAMPLE_RATE_HZ, 500);
        let sr = KNOCK_ADC_SAMPLE_RATE_HZ;
        let mut buf = Vec::new();
        for i in 0..FFT_SIZE * 3 {
            let t = i as f32 / sr;
            let v = (2.0 * PI * 8000.0 * t).sin() * 2000.0 + 2000.0;
            buf.push(v);
        }
        eng.push_samples(&buf);
        let view = eng.view();
        assert!(view.width >= 1);
        assert_eq!(view.pixels.len(), view.width * view.height);
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
    fn run_peak_is_max_over_entire_run_not_sliding_window() {
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

        // Заполняем окно слабым шумом — столбцы с пиками уходят из deque.
        for _ in 0..80 {
            let mut quiet = vec![0u8; h];
            quiet[3] = 10;
            eng.feed_test_column(quiet);
        }

        assert_eq!(
            eng.peak_frequency_hz(),
            Some(run_peak_hz),
            "пик прогона не должен сбрасываться при прокрутке окна"
        );
    }
}

#[cfg(test)]
impl KnockSpectrogramEngine {
    fn feed_test_column(&mut self, col: Vec<u8>) {
        self.note_column_peak(&col);
        self.pending_new_columns.extend_from_slice(&col);
        self.columns.push_back(col);
        while self.columns.len() > self.max_columns {
            self.columns.pop_front();
        }
    }
}
