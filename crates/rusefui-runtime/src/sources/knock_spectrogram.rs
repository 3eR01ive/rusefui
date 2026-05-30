//! FFT-спектрограмма knock scope на хосте (как `software_knock.cpp` + `fft.hpp`).

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::Fft;
use serde::Serialize;

pub const KNOCK_ADC_SAMPLE_RATE_HZ: f32 = 218_750.0;

pub const FFT_SIZE: usize = 1024;
pub const HOP: usize = 256;
pub const START_FREQ_HZ: f32 = 4000.0;
pub const NUM_BINS: usize = 64;
const ADC_RATIO: f32 = 3.3 / 4095.0;
const SENSITIVITY: f32 = 1.0;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnockSpectrogramView {
    pub width: usize,
    pub height: usize,
    pub freq_start_hz: f32,
    pub freq_step_hz: f32,
    /// column-major: index = time_col * height + freq_bin
    pub pixels: Vec<u8>,
}

pub struct KnockSpectrogramEngine {
    _sample_rate_hz: f32,
    max_columns: usize,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    stream: Vec<f32>,
    stream_offset: usize,
    columns: VecDeque<Vec<u8>>,
    start_bin: usize,
    freq_start_hz: f32,
    freq_step_hz: f32,
}

impl KnockSpectrogramEngine {
    pub fn new(sample_rate_hz: f32, window_ms: u32) -> Self {
        let max_columns = max_columns_for_window(window_ms, sample_rate_hz);
        let mut planner = rustfft::FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let window = blackman_harris(FFT_SIZE, true);
        let (start_bin, freq_start_hz, freq_step_hz) = spectrogram_bin_layout(sample_rate_hz);

        Self {
            _sample_rate_hz: sample_rate_hz,
            max_columns,
            fft,
            window,
            scratch: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            stream: Vec::new(),
            stream_offset: 0,
            columns: VecDeque::new(),
            start_bin,
            freq_start_hz,
            freq_step_hz,
        }
    }

    pub fn clear(&mut self) {
        self.stream.clear();
        self.stream_offset = 0;
        self.columns.clear();
    }

    pub fn push_samples(&mut self, samples: &[f32]) {
        self.stream.extend_from_slice(samples);
        self.drain_fft_windows();
        self.trim_stream();
    }

    pub fn view(&self) -> KnockSpectrogramView {
        let height = NUM_BINS;
        let width = self.columns.len();
        if width == 0 {
            return KnockSpectrogramView {
                width: 0,
                height,
                freq_start_hz: self.freq_start_hz,
                freq_step_hz: self.freq_step_hz,
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
            freq_start_hz: self.freq_start_hz,
            freq_step_hz: self.freq_step_hz,
            pixels,
        }
    }

    fn drain_fft_windows(&mut self) {
        while self.stream.len().saturating_sub(self.stream_offset) >= FFT_SIZE {
            let start = self.stream_offset;
            let frame = &self.stream[start..start + FFT_SIZE];

            for (i, &adc) in frame.iter().enumerate() {
                let voltage = ADC_RATIO * adc;
                self.scratch[i] = Complex::new(SENSITIVITY * voltage * self.window[i], 0.0);
            }
            for i in FFT_SIZE..self.scratch.len() {
                self.scratch[i] = Complex::new(0.0, 0.0);
            }

            self.fft.process(&mut self.scratch);

            let col = spectrum_column(&self.scratch, self.start_bin);
            self.columns.push_back(col);
            while self.columns.len() > self.max_columns {
                self.columns.pop_front();
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

fn spectrogram_bin_layout(sample_rate_hz: f32) -> (usize, f32, f32) {
    let mut best_i = 0usize;
    let mut best_diff = f32::MAX;
    let half = FFT_SIZE / 2;
    for i in 0..half {
        let freq = i as f32 * sample_rate_hz / FFT_SIZE as f32;
        let diff = (freq - START_FREQ_HZ).abs();
        if diff < best_diff {
            best_diff = diff;
            best_i = i;
        }
    }
    let freq_start = best_i as f32 * sample_rate_hz / FFT_SIZE as f32;
    let next_freq = (best_i + 1) as f32 * sample_rate_hz / FFT_SIZE as f32;
    (best_i, freq_start, next_freq - freq_start)
}

fn spectrum_column(scratch: &[Complex<f32>], start_bin: usize) -> Vec<u8> {
    let mut col = Vec::with_capacity(NUM_BINS);
    for i in 0..NUM_BINS {
        let idx = start_bin + i * 4;
        let mut peak = 0.0f32;
        for k in 0..4 {
            let c = &scratch[idx + k];
            let amp = (c.re * c.re + c.im * c.im).sqrt();
            peak = peak.max(amp);
        }
        col.push(amplitude_to_db(peak));
    }
    col
}

/// Частота пика по heatmap (максимальная амплитуда среди всех колонок).
pub fn peak_frequency_hz(view: &KnockSpectrogramView) -> Option<f32> {
    if view.width == 0 || view.height == 0 || view.pixels.is_empty() {
        return None;
    }
    let mut best_val = 0u8;
    let mut best_row = 0usize;
    for col in 0..view.width {
        for row in 0..view.height {
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
    Some(view.freq_start_hz + best_row as f32 * view.freq_step_hz)
}

fn amplitude_to_db(amplitude: f32) -> u8 {
    let v = amplitude.max(1e-12);
    let db = 200.0 * (v * v).log10() + 40.0;
    db.clamp(0.0, 255.0) as u8
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
    fn bin_layout_near_4khz() {
        let (start, f0, step) = spectrogram_bin_layout(KNOCK_ADC_SAMPLE_RATE_HZ);
        assert!(start > 0);
        assert!((f0 - START_FREQ_HZ).abs() < 500.0);
        assert!(step > 0.0);
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
        assert_eq!(view.pixels.len(), view.width * NUM_BINS);
    }
}
