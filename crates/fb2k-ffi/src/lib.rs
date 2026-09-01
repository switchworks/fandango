//! C ABI for foobar2000 native DSP (spread + nice-bulk).
//!
//! DSP cores are the same sources as VST3 (path / build.rs patch). Do not edit
//! those crate files from here.

#![allow(clippy::missing_safety_doc)]

// Path is relative to this file (lib.rs), not a nested folder.
#[path = "../../shared/src/biquad.rs"]
mod biquad_src;

mod shared_dsp {
    pub use crate::biquad_src::Biquad;
    include!(concat!(env!("OUT_DIR"), "/lr4_patched.rs"));
}

#[allow(dead_code)]
#[path = "../../spread/src/dsp.rs"]
mod spread_dsp;

#[allow(dead_code)]
#[path = "../../crush/src/dsp.rs"]
mod crush_dsp;

#[allow(dead_code)]
#[path = "../../tilt/src/dsp.rs"]
mod tilt_dsp;

#[allow(dead_code)]
#[path = "../../finalist/src/dsp.rs"]
mod finalist_dsp;

#[allow(dead_code)]
mod nice_bulk_dsp {
    include!(concat!(env!("OUT_DIR"), "/nice_bulk_dsp_patched.rs"));
}

use crush_dsp::CrushChannel;
use finalist_dsp::Finalist as FinalistCore;
use nice_bulk_dsp::NiceBulkChannel;
use spread_dsp::Coefs;
use tilt_dsp::{high_shelf_coeffs, low_shelf_coeffs, BiquadCoeffs, TiltChannel};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// spread
// ---------------------------------------------------------------------------

pub struct FdgSpread {
    srate: f32,
    bypass: bool,
    expander: f32,
    inverse: bool,
    center_amount: f32,
}

impl FdgSpread {
    fn new(srate: f32) -> Self {
        Self {
            srate: if srate.is_finite() && srate > 0.0 {
                srate
            } else {
                48_000.0
            },
            bypass: false,
            expander: 0.66,
            inverse: false,
            center_amount: 1.0,
        }
    }

    fn process_buffer(&self, left: &mut [f32], right: &mut [f32]) {
        if self.bypass {
            return;
        }
        debug_assert_eq!(left.len(), right.len());
        let k = Coefs::from_params(self.expander, self.center_amount);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (lin, rin) = if self.inverse { (*r, *l) } else { (*l, *r) };
            *l = k.a * lin + k.c * rin;
            *r = k.c * lin + k.a * rin;
        }
    }
}

#[no_mangle]
pub extern "C" fn fdg_spread_new(srate: f32) -> *mut FdgSpread {
    match catch_unwind(|| Box::into_raw(Box::new(FdgSpread::new(srate)))) {
        Ok(p) => p,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdg_spread_free(p: *mut FdgSpread) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(Box::from_raw(p));
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_spread_set_params(
    p: *mut FdgSpread,
    bypass: i32,
    expander: f32,
    inverse: i32,
    center_amount: f32,
) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inst = &mut *p;
        inst.bypass = bypass != 0;
        inst.expander = if expander.is_finite() {
            expander.clamp(0.0, 1.0)
        } else {
            0.5
        };
        inst.inverse = inverse != 0;
        inst.center_amount = if center_amount.is_finite() {
            center_amount.clamp(0.0, 1.0)
        } else {
            1.0
        };
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_spread_reset(p: *mut FdgSpread, srate: f32) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inst = &mut *p;
        if srate.is_finite() && srate > 0.0 {
            inst.srate = srate;
        }
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_spread_process(
    p: *mut FdgSpread,
    left: *mut f32,
    right: *mut f32,
    frames: usize,
) {
    if p.is_null() || left.is_null() || right.is_null() || frames == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inst = &*p;
        let l = std::slice::from_raw_parts_mut(left, frames);
        let r = std::slice::from_raw_parts_mut(right, frames);
        inst.process_buffer(l, r);
    }));
}

// ---------------------------------------------------------------------------
// nice-bulk
// ---------------------------------------------------------------------------

/// fb2k listening defaults (VST defaults are 0.0 = identity). Provisional.
pub const FDG_NICE_DEFAULT_BASS: f32 = 0.30;
pub const FDG_NICE_DEFAULT_HI: f32 = 0.30;

pub struct FdgNiceBulk {
    srate: f32,
    channels: Vec<NiceBulkChannel>,
    bypass: bool,
    bass_amount: f32,
    hi_amount: f32,
}

impl FdgNiceBulk {
    fn new(srate: f32, channel_count: usize) -> Self {
        let srate = if srate.is_finite() && srate > 0.0 {
            srate
        } else {
            48_000.0
        };
        let n = channel_count.max(1);
        Self {
            srate,
            channels: (0..n).map(|_| NiceBulkChannel::new(srate)).collect(),
            bypass: false,
            bass_amount: FDG_NICE_DEFAULT_BASS,
            hi_amount: FDG_NICE_DEFAULT_HI,
        }
    }

    fn ensure_layout(&mut self, srate: f32, channel_count: usize) {
        let srate = if srate.is_finite() && srate > 0.0 {
            srate
        } else {
            self.srate
        };
        let n = channel_count.max(1);
        if (srate - self.srate).abs() > 0.5 || self.channels.len() != n {
            self.srate = srate;
            self.channels = (0..n).map(|_| NiceBulkChannel::new(srate)).collect();
        }
    }

    fn reset_state(&mut self, srate: f32) {
        if srate.is_finite() && srate > 0.0 {
            self.srate = srate;
        }
        for ch in &mut self.channels {
            ch.set_sr(self.srate);
        }
    }

    fn process_interleaved(&mut self, data: &mut [f32], ch: usize, frames: usize) {
        if self.bypass || ch == 0 || frames == 0 {
            return;
        }
        self.ensure_layout(self.srate, ch);
        let bass = self.bass_amount;
        let hi = self.hi_amount;
        for f in 0..frames {
            for c in 0..ch {
                let i = f * ch + c;
                data[i] = self.channels[c].process(data[i], bass, hi);
            }
        }
    }
}

#[inline]
fn enable_ftz_daz() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        let csr = _mm_getcsr();
        _mm_setcsr(csr | 0x8040);
    }
}

#[no_mangle]
pub extern "C" fn fdg_nice_new(srate: f32, channel_count: u32) -> *mut FdgNiceBulk {
    match catch_unwind(|| {
        Box::into_raw(Box::new(FdgNiceBulk::new(
            srate,
            channel_count as usize,
        )))
    }) {
        Ok(p) => p,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdg_nice_free(p: *mut FdgNiceBulk) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(Box::from_raw(p));
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_nice_set_params(
    p: *mut FdgNiceBulk,
    bypass: i32,
    bass_amount: f32,
    hi_amount: f32,
) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inst = &mut *p;
        inst.bypass = bypass != 0;
        inst.bass_amount = if bass_amount.is_finite() {
            bass_amount.clamp(0.0, 1.0)
        } else {
            FDG_NICE_DEFAULT_BASS
        };
        inst.hi_amount = if hi_amount.is_finite() {
            hi_amount.clamp(0.0, 1.0)
        } else {
            FDG_NICE_DEFAULT_HI
        };
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_nice_reset(p: *mut FdgNiceBulk, srate: f32) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        (*p).reset_state(srate);
    }));
}

/// Interleaved process. `data` length >= `frames * channel_count`.
#[no_mangle]
pub unsafe extern "C" fn fdg_nice_process(
    p: *mut FdgNiceBulk,
    data: *mut f32,
    channel_count: u32,
    frames: usize,
) {
    if p.is_null() || data.is_null() || channel_count == 0 || frames == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        enable_ftz_daz();
        let inst = &mut *p;
        let ch = channel_count as usize;
        let slice = std::slice::from_raw_parts_mut(data, frames * ch);
        inst.process_interleaved(slice, ch, frames);
    }));
}

// ---------------------------------------------------------------------------
// crush — listening defaults (VST: bits=8 rate=1 mix=1.0)
// ---------------------------------------------------------------------------

pub const FDG_CRUSH_DEFAULT_BITS: f32 = 10.0;
pub const FDG_CRUSH_DEFAULT_RATE: f32 = 2.0;
pub const FDG_CRUSH_DEFAULT_MIX: f32 = 0.40;

pub struct FdgCrush {
    channels: Vec<CrushChannel>,
    bypass: bool,
    bits: f32,
    /// S&H ダウンサンプル係数の**実係数**（1.0..=32.0、実数）。
    /// VST3 側のパラメータは `log2(rate)` で持つが、FFI は DSP の言語で切る
    /// （`plans/crush_rate_fb2k_v01.md` §2-1）。呼び出し側が `2^p` を済ませて渡す。
    rate: f32,
    mix: f32,
}

impl FdgCrush {
    fn new(channel_count: usize) -> Self {
        let n = channel_count.max(1);
        Self {
            channels: (0..n).map(|_| CrushChannel::new()).collect(),
            bypass: false,
            bits: FDG_CRUSH_DEFAULT_BITS,
            rate: FDG_CRUSH_DEFAULT_RATE,
            mix: FDG_CRUSH_DEFAULT_MIX,
        }
    }

    fn ensure_channels(&mut self, channel_count: usize) {
        let n = channel_count.max(1);
        if self.channels.len() != n {
            self.channels = (0..n).map(|_| CrushChannel::new()).collect();
        }
    }

    fn reset_state(&mut self) {
        for ch in &mut self.channels {
            ch.reset();
        }
    }

    fn process_interleaved(&mut self, data: &mut [f32], ch: usize, frames: usize) {
        if self.bypass || ch == 0 || frames == 0 {
            return;
        }
        self.ensure_channels(ch);
        let bits = self.bits;
        let rate = self.rate;
        let mix = self.mix;
        for f in 0..frames {
            for c in 0..ch {
                let i = f * ch + c;
                data[i] = self.channels[c].process(data[i], bits, rate, mix);
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn fdg_crush_new(channel_count: u32) -> *mut FdgCrush {
    match catch_unwind(|| Box::into_raw(Box::new(FdgCrush::new(channel_count as usize)))) {
        Ok(p) => p,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdg_crush_free(p: *mut FdgCrush) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(Box::from_raw(p));
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_crush_set_params(
    p: *mut FdgCrush,
    bypass: i32,
    bits: f32,
    rate: f32,
    mix: f32,
) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inst = &mut *p;
        inst.bypass = bypass != 0;
        inst.bits = if bits.is_finite() {
            bits.clamp(1.0, 16.0)
        } else {
            FDG_CRUSH_DEFAULT_BITS
        };
        inst.rate = if rate.is_finite() {
            rate.clamp(1.0, 32.0)
        } else {
            FDG_CRUSH_DEFAULT_RATE
        };
        inst.mix = if mix.is_finite() {
            mix.clamp(0.0, 1.0)
        } else {
            FDG_CRUSH_DEFAULT_MIX
        };
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_crush_reset(p: *mut FdgCrush) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        (*p).reset_state();
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_crush_process(
    p: *mut FdgCrush,
    data: *mut f32,
    channel_count: u32,
    frames: usize,
) {
    if p.is_null() || data.is_null() || channel_count == 0 || frames == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        enable_ftz_daz();
        let inst = &mut *p;
        let ch = channel_count as usize;
        let slice = std::slice::from_raw_parts_mut(data, frames * ch);
        inst.process_interleaved(slice, ch, frames);
    }));
}

// ---------------------------------------------------------------------------
// tilt — listening defaults (VST gain=0 identity)
// ---------------------------------------------------------------------------

pub const FDG_TILT_DEFAULT_GAIN: f32 = 1.5;
pub const FDG_TILT_DEFAULT_PIVOT: f32 = 700.0;
pub const FDG_TILT_DEFAULT_SLOPE: f32 = 0.7;

pub struct FdgTilt {
    srate: f32,
    channels: Vec<TiltChannel>,
    ls: BiquadCoeffs,
    hs: BiquadCoeffs,
    bypass: bool,
    gain: f32,
    pivot: f32,
    slope: f32,
    dirty: bool,
}

impl FdgTilt {
    fn new(srate: f32, channel_count: usize) -> Self {
        let srate = if srate.is_finite() && srate > 0.0 {
            srate
        } else {
            48_000.0
        };
        let gain = FDG_TILT_DEFAULT_GAIN;
        let pivot = FDG_TILT_DEFAULT_PIVOT;
        let slope = FDG_TILT_DEFAULT_SLOPE;
        let n = channel_count.max(1);
        Self {
            srate,
            channels: (0..n).map(|_| TiltChannel::new()).collect(),
            ls: low_shelf_coeffs(srate, pivot, -gain, slope),
            hs: high_shelf_coeffs(srate, pivot, gain, slope),
            bypass: false,
            gain,
            pivot,
            slope,
            dirty: false,
        }
    }

    fn recompute(&mut self) {
        self.ls = low_shelf_coeffs(self.srate, self.pivot, -self.gain, self.slope);
        self.hs = high_shelf_coeffs(self.srate, self.pivot, self.gain, self.slope);
        self.dirty = false;
    }

    fn ensure_layout(&mut self, srate: f32, channel_count: usize) {
        let srate = if srate.is_finite() && srate > 0.0 {
            srate
        } else {
            self.srate
        };
        let n = channel_count.max(1);
        let sr_changed = (srate - self.srate).abs() > 0.5;
        if sr_changed || self.channels.len() != n {
            self.srate = srate;
            self.channels = (0..n).map(|_| TiltChannel::new()).collect();
            self.dirty = true;
        }
        if self.dirty {
            self.recompute();
        }
    }

    fn reset_state(&mut self, srate: f32) {
        if srate.is_finite() && srate > 0.0 {
            self.srate = srate;
        }
        for ch in &mut self.channels {
            ch.reset();
        }
        self.recompute();
    }

    fn process_interleaved(&mut self, data: &mut [f32], ch: usize, frames: usize) {
        if self.bypass || ch == 0 || frames == 0 {
            return;
        }
        self.ensure_layout(self.srate, ch);
        let ls = self.ls;
        let hs = self.hs;
        for f in 0..frames {
            for c in 0..ch {
                let i = f * ch + c;
                data[i] = self.channels[c].process(data[i], &ls, &hs);
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn fdg_tilt_new(srate: f32, channel_count: u32) -> *mut FdgTilt {
    match catch_unwind(|| {
        Box::into_raw(Box::new(FdgTilt::new(srate, channel_count as usize)))
    }) {
        Ok(p) => p,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdg_tilt_free(p: *mut FdgTilt) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(Box::from_raw(p));
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_tilt_set_params(
    p: *mut FdgTilt,
    bypass: i32,
    gain: f32,
    pivot: f32,
    slope: f32,
) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inst = &mut *p;
        inst.bypass = bypass != 0;
        let g = if gain.is_finite() {
            gain.clamp(-12.0, 12.0)
        } else {
            FDG_TILT_DEFAULT_GAIN
        };
        let pv = if pivot.is_finite() {
            pivot.clamp(100.0, 5000.0)
        } else {
            FDG_TILT_DEFAULT_PIVOT
        };
        let sl = if slope.is_finite() {
            slope.clamp(0.3, 1.0)
        } else {
            FDG_TILT_DEFAULT_SLOPE
        };
        if (g - inst.gain).abs() > 1e-6
            || (pv - inst.pivot).abs() > 1e-3
            || (sl - inst.slope).abs() > 1e-6
        {
            inst.gain = g;
            inst.pivot = pv;
            inst.slope = sl;
            inst.dirty = true;
        }
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_tilt_reset(p: *mut FdgTilt, srate: f32) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        (*p).reset_state(srate);
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_tilt_process(
    p: *mut FdgTilt,
    data: *mut f32,
    channel_count: u32,
    frames: usize,
) {
    if p.is_null() || data.is_null() || channel_count == 0 || frames == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        enable_ftz_daz();
        let inst = &mut *p;
        let ch = channel_count as usize;
        let slice = std::slice::from_raw_parts_mut(data, frames * ch);
        inst.process_interleaved(slice, ch, frames);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spread_smoke() {
        let p = fdg_spread_new(48_000.0);
        assert!(!p.is_null());
        unsafe {
            fdg_spread_set_params(p, 0, 0.0, 0, 1.0);
            let n = 64usize;
            let mut left = vec![1.0f32; n];
            let mut right = vec![0.0f32; n];
            fdg_spread_process(p, left.as_mut_ptr(), right.as_mut_ptr(), n);
            assert!((left[0] - 2.0 / 3.0).abs() < 1e-5);
            fdg_spread_free(p);
        }
    }

    #[test]
    fn nice_zero_stays_finite_and_stable() {
        let p = fdg_nice_new(48_000.0, 1);
        assert!(!p.is_null());
        unsafe {
            fdg_nice_set_params(p, 0, 0.0, 0.0);
            let n = 2048usize;
            let mut data: Vec<f32> = (0..n)
                .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 250.0 * (i as f32) / 48_000.0).sin())
                .collect();
            fdg_nice_process(p, data.as_mut_ptr(), 1, n);
            assert!(data.iter().all(|x| x.is_finite()));
            // amount=0 is gain-identity but LR4 still shapes; energy should remain.
            let energy: f32 = data.iter().map(|x| x * x).sum();
            assert!(energy > 1.0, "energy={energy}");
            fdg_nice_free(p);
        }
    }

    #[test]
    fn nice_default_changes_low_level_bass() {
        let p = fdg_nice_new(48_000.0, 1);
        unsafe {
            fdg_nice_set_params(p, 0, FDG_NICE_DEFAULT_BASS, 0.0);
            let n = 4800usize;
            let mut data: Vec<f32> = (0..n)
                .map(|i| 0.01 * (2.0 * std::f32::consts::PI * 40.0 * (i as f32) / 48_000.0).sin())
                .collect();
            let peak_in = data.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            fdg_nice_process(p, data.as_mut_ptr(), 1, n);
            let peak_out = data[n / 2..].iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            assert!(peak_out > peak_in * 1.1, "peak_in={peak_in} peak_out={peak_out}");
            fdg_nice_free(p);
        }
    }

    #[test]
    fn crush_mix_changes_signal() {
        let p = fdg_crush_new(1);
        unsafe {
            fdg_crush_set_params(p, 0, 4.0, 1.0, 1.0);
            let n = 256usize;
            let mut data: Vec<f32> = (0..n)
                .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / 48_000.0).sin())
                .collect();
            let before = data.clone();
            fdg_crush_process(p, data.as_mut_ptr(), 1, n);
            assert!(data.iter().all(|x| x.is_finite()));
            let diff: f32 = data
                .iter()
                .zip(before.iter())
                .map(|(a, b)| (a - b).abs())
                .sum();
            assert!(diff > 0.1, "diff={diff}");
            fdg_crush_free(p);
        }
    }

    #[test]
    fn tilt_gain_changes_highs() {
        let p = fdg_tilt_new(48_000.0, 1);
        unsafe {
            fdg_tilt_set_params(p, 0, 6.0, 700.0, 0.7);
            let n = 2048usize;
            let mut data: Vec<f32> = (0..n)
                .map(|i| 0.2 * (2.0 * std::f32::consts::PI * 4000.0 * (i as f32) / 48_000.0).sin())
                .collect();
            let peak_in = data.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            fdg_tilt_process(p, data.as_mut_ptr(), 1, n);
            let peak_out = data[n / 2..].iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            assert!(data.iter().all(|x| x.is_finite()));
            assert!(peak_out > peak_in * 1.2, "peak_in={peak_in} peak_out={peak_out}");
            fdg_tilt_free(p);
        }
    }
}

// ---------------------------------------------------------------------------
// finalist — listening defaults
//
// input のみ fb2k 既定 (+6 dB、再生時の利便性)。drive / ceiling / release は
// VST3 と同一値を使う。
// ---------------------------------------------------------------------------

pub const FDG_FINALIST_DEFAULT_INPUT_DB: f32 = 6.0;
pub const FDG_FINALIST_DEFAULT_DRIVE: f32 = 0.0;
pub const FDG_FINALIST_DEFAULT_CEILING_DB: f32 = -1.0;
pub const FDG_FINALIST_DEFAULT_RELEASE_MS: f32 = 80.0;

#[inline]
fn db_to_gain(db: f32) -> f32 {
    10.0f32.powf(db * 0.05)
}

/// 多チャンネルは 2ch ずつ独立インスタンスへ割り当てる。ペア内はステレオリンク、
/// ペア間はリンクしない（fb2k のサラウンド再生は稀なため、単純さを優先）。
/// 奇数チャンネルの余りは右へ 0.0 を与え、左出力のみ使う。
pub struct FdgFinalist {
    srate: f32,
    ch: usize,
    pairs: Vec<FinalistCore>,
    bypass: bool,
    input_gain: f32,
    drive: f32,
    ceiling: f32,
    release_ms: f32,
}

impl FdgFinalist {
    fn new(srate: f32, channel_count: usize) -> Self {
        let srate = if srate.is_finite() && srate > 0.0 {
            srate
        } else {
            48_000.0
        };
        let ch = channel_count.max(1);
        let pairs = (0..ch.div_ceil(2))
            .map(|_| FinalistCore::new(srate))
            .collect();
        Self {
            srate,
            ch,
            pairs,
            bypass: false,
            input_gain: db_to_gain(FDG_FINALIST_DEFAULT_INPUT_DB),
            drive: FDG_FINALIST_DEFAULT_DRIVE,
            ceiling: db_to_gain(FDG_FINALIST_DEFAULT_CEILING_DB),
            release_ms: FDG_FINALIST_DEFAULT_RELEASE_MS,
        }
    }

    fn ensure_layout(&mut self, srate: f32, channel_count: usize) {
        let srate = if srate.is_finite() && srate > 0.0 {
            srate
        } else {
            self.srate
        };
        let ch = channel_count.max(1);
        if (srate - self.srate).abs() > 0.5 || self.ch != ch {
            self.srate = srate;
            self.ch = ch;
            self.pairs = (0..ch.div_ceil(2))
                .map(|_| FinalistCore::new(srate))
                .collect();
        }
    }

    fn reset_state(&mut self, srate: f32) {
        if srate.is_finite() && srate > 0.0 && (srate - self.srate).abs() > 0.5 {
            self.srate = srate;
            self.pairs = (0..self.ch.div_ceil(2))
                .map(|_| FinalistCore::new(srate))
                .collect();
            return;
        }
        for p in &mut self.pairs {
            p.reset();
        }
    }

    fn process_interleaved(&mut self, data: &mut [f32], ch: usize, frames: usize) {
        if ch == 0 || frames == 0 {
            return;
        }
        self.ensure_layout(self.srate, ch);
        // ceiling / release は窓内一定であること（天井保証の前提）。ブロック単位で固定する。
        let rel = self.pairs[0].release_coeff(self.release_ms);
        let (input_gain, drive, ceiling, bypass) =
            (self.input_gain, self.drive, self.ceiling, self.bypass);
        for f in 0..frames {
            let base = f * ch;
            for (pi, core) in self.pairs.iter_mut().enumerate() {
                let li = base + pi * 2;
                let ri = li + 1;
                let has_r = pi * 2 + 1 < ch;
                let r_in = if has_r { data[ri] } else { 0.0 };
                let (ol, or) =
                    core.process(data[li], r_in, input_gain, drive, ceiling, rel, bypass);
                data[li] = ol;
                if has_r {
                    data[ri] = or;
                }
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn fdg_finalist_new(srate: f32, channel_count: u32) -> *mut FdgFinalist {
    match catch_unwind(|| {
        Box::into_raw(Box::new(FdgFinalist::new(srate, channel_count as usize)))
    }) {
        Ok(p) => p,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdg_finalist_free(p: *mut FdgFinalist) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(Box::from_raw(p));
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_finalist_set_params(
    p: *mut FdgFinalist,
    bypass: i32,
    input_db: f32,
    drive: f32,
    ceiling_db: f32,
    release_ms: f32,
) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inst = &mut *p;
        inst.bypass = bypass != 0;
        let idb = if input_db.is_finite() {
            input_db.clamp(-12.0, 12.0)
        } else {
            FDG_FINALIST_DEFAULT_INPUT_DB
        };
        inst.input_gain = db_to_gain(idb);
        inst.drive = if drive.is_finite() {
            drive.clamp(0.0, 1.0)
        } else {
            FDG_FINALIST_DEFAULT_DRIVE
        };
        let cdb = if ceiling_db.is_finite() {
            ceiling_db.clamp(-12.0, 0.0)
        } else {
            FDG_FINALIST_DEFAULT_CEILING_DB
        };
        inst.ceiling = db_to_gain(cdb);
        inst.release_ms = if release_ms.is_finite() {
            release_ms.clamp(10.0, 500.0)
        } else {
            FDG_FINALIST_DEFAULT_RELEASE_MS
        };
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_finalist_reset(p: *mut FdgFinalist, srate: f32) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        (*p).reset_state(srate);
    }));
}

/// ゲイン経路の総群遅延（サンプル）。tail flush 長と `get_latency` はこの値で決める。
#[no_mangle]
pub unsafe extern "C" fn fdg_finalist_latency(p: *mut FdgFinalist) -> u32 {
    if p.is_null() {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        let inst = &*p;
        inst.pairs[0].latency_samples()
    }))
    .unwrap_or(0)
}

/// Interleaved process. `data` length >= `frames * channel_count`.
#[no_mangle]
pub unsafe extern "C" fn fdg_finalist_process(
    p: *mut FdgFinalist,
    data: *mut f32,
    channel_count: u32,
    frames: usize,
) {
    if p.is_null() || data.is_null() || channel_count == 0 || frames == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        enable_ftz_daz();
        let inst = &mut *p;
        let ch = channel_count as usize;
        let slice = std::slice::from_raw_parts_mut(data, frames * ch);
        inst.process_interleaved(slice, ch, frames);
    }));

    #[test]
    fn finalist_ceiling_is_never_exceeded() {
        let p = fdg_finalist_new(48_000.0, 2);
        assert!(!p.is_null());
        unsafe {
            let ceiling_db = FDG_FINALIST_DEFAULT_CEILING_DB;
            fdg_finalist_set_params(p, 0, 12.0, 1.0, ceiling_db, 80.0);
            assert!(fdg_finalist_latency(p) > 0);
            let frames = 48_000usize;
            let mut data = vec![0.0f32; frames * 2];
            for i in 0..frames {
                let t = i as f32 / 48_000.0;
                let v = 0.9 * (2.0 * std::f32::consts::PI * 110.0 * t).sin()
                    + 0.5 * (2.0 * std::f32::consts::PI * 3000.0 * t).sin();
                data[i * 2] = v;
                data[i * 2 + 1] = -v;
            }
            fdg_finalist_process(p, data.as_mut_ptr(), 2, frames);
            let ceiling = db_to_gain(ceiling_db);
            let peak = data.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
            assert!(data.iter().all(|x| x.is_finite()));
            assert!(peak <= ceiling + 1e-6, "peak={peak} ceiling={ceiling}");
            fdg_finalist_free(p);
        }
    }

    #[test]
    fn finalist_mono_odd_channel_is_finite() {
        let p = fdg_finalist_new(44_100.0, 1);
        unsafe {
            let frames = 1024usize;
            let mut data: Vec<f32> = (0..frames)
                .map(|i| 0.8 * (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / 44_100.0).sin())
                .collect();
            fdg_finalist_process(p, data.as_mut_ptr(), 1, frames);
            assert!(data.iter().all(|x| x.is_finite()));
            fdg_finalist_free(p);
        }
    }
}
