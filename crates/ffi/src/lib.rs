//! C ABI exposing the fandango DSP cores to hosts that are not VST3.
//!
//! Consumers: the foobar2000 component in `fb2k/`, and the players built on
//! top of this ABI. The name is deliberately consumer-neutral — it was
//! `fb2k-ffi` until 2026-11-23, which read as if foobar2000 were the only one.
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
}

#[cfg(test)]
mod finalist_tests {
    //! この 2 本は `fdg_finalist_process` の**本体の中**に `#[test]` として
    //! 置かれていて、テストハーネスに収集されていなかった
    //! （rustc は `never used` で警告していた。実測 2026-11-24: 全 77 本のどこにも
    //! `finalist` が出ない）。module へ出して収集されるようにした。
    //! **中身は 1 文字も変えていない。**

    use super::*;

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

// ---------------------------------------------------------------------------
// chain — FFI 境界 L1（`plans/ios_player_v02.md` §3-2・I6・I7・I11）
//
// **追加のみ**（I2）。上の 26 関数は 1 つも触っていない。fb2k 版が
// `include/fdg_ffi.h` 越しに実機でそれを呼んでいる。
//
// L1 で Rust 側が負う責任は 4 つ:
//   - チェーン結線（I6 の固定順。並べ替えは提供しない）
//   - パラメータ状態（レート / チャンネル変更の作り直しをまたいで保持する）
//   - レイテンシの出所（F16。`fdg_chain_latency` が finalist から再取得する）
//   - レート / チャンネル変更時の作り直し手順（I7。`fdg_chain_prepare`）
//
// プラットフォーム側（Swift / Kotlin）の義務は
// 「曲の切り替え時に**非 RT スレッドで** `fdg_chain_prepare` を 1 回呼び、
// 返ってきたレイテンシを表示と tail flush に使う」だけ。I6 / I7 / F16 を
// 書き写さない（2 言語に分かれて食い違うのを防ぐのが L1 の目的）。
// ---------------------------------------------------------------------------

/// `fdg_*_set_params` に渡した値をそのまま保持する。I7 の作り直しの後に
/// 流し直すために要る（L1 の責任 #8）。
#[derive(Clone, Copy)]
struct TiltParams {
    bypass: i32,
    gain: f32,
    pivot: f32,
    slope: f32,
}

#[derive(Clone, Copy)]
struct NiceParams {
    bypass: i32,
    bass_amount: f32,
    hi_amount: f32,
}

#[derive(Clone, Copy)]
struct SpreadParams {
    bypass: i32,
    expander: f32,
    inverse: i32,
    center_amount: f32,
}

#[derive(Clone, Copy)]
struct CrushParams {
    bypass: i32,
    bits: f32,
    rate: f32,
    mix: f32,
}

#[derive(Clone, Copy)]
struct FinalistParams {
    bypass: i32,
    input_db: f32,
    drive: f32,
    ceiling_db: f32,
    release_ms: f32,
}

impl FdgSpread {
    /// インターリーブ 2ch をインプレースで処理する。
    ///
    /// 式と評価順を [`FdgSpread::process_buffer`]（planar L/R）と揃えてある。
    /// **ずれていないことは `chain_matches_individual_calls` が f32 の等値で見る**
    /// （`fdg_spread_process` を planar に呼んだ結果とビット一致）。
    fn process_interleaved_stereo(&self, data: &mut [f32], frames: usize) {
        if self.bypass {
            return;
        }
        let k = Coefs::from_params(self.expander, self.center_amount);
        for f in 0..frames {
            let i = f * 2;
            let (l, r) = (data[i], data[i + 1]);
            let (lin, rin) = if self.inverse { (r, l) } else { (l, r) };
            data[i] = k.a * lin + k.c * rin;
            data[i + 1] = k.c * lin + k.a * rin;
        }
    }
}

/// I6 の 5 段を固定順で持つ。各段は上の `Fdg*` をそのまま所有する
/// （chain 専用に DSP を書き写さない。ビット一致を構造で保証するため）。
pub struct FdgChain {
    srate: f32,
    ch: usize,
    tilt: FdgTilt,
    nice: FdgNiceBulk,
    spread: FdgSpread,
    crush: FdgCrush,
    finalist: FdgFinalist,
    p_tilt: TiltParams,
    p_nice: NiceParams,
    p_spread: SpreadParams,
    p_crush: CrushParams,
    p_finalist: FinalistParams,
}

impl FdgChain {
    /// 初期状態は**全段 bypass**（crush は `mix = 0` の厳密恒等）。
    /// つまり作った直後の chain は、finalist のレイテンシ分だけ遅れた入力そのもの。
    ///
    /// 各段の値は `Fdg*::new` の既定のままだが、**3 つだけ意図的に外している**。
    /// bypass を外した瞬間に音が飛ばないようにするため:
    ///   - crush `mix`: 0.40 → **0.0**（`plans/ios_player_v02.md` Step 11c の推奨）
    ///   - finalist `input_db`: +6.0 → **0.0**（+6 は fb2k のリスニング既定）
    ///   - spread `expander`: 0.66 → **0.5**（a=1 / c=0 の恒等点）
    ///
    /// **リスニング既定の裁定は Step 11c（オーナーの耳）。ここでは決めない。**
    /// プラットフォーム側は描画の前に 5 本の setter を明示的に呼ぶ。
    fn new(srate: f32, channel_count: usize) -> Self {
        let srate = if srate.is_finite() && srate > 0.0 {
            srate
        } else {
            48_000.0
        };
        let ch = channel_count.max(1);
        let mut inst = Self {
            srate,
            ch,
            tilt: FdgTilt::new(srate, ch),
            nice: FdgNiceBulk::new(srate, ch),
            spread: FdgSpread::new(srate),
            crush: FdgCrush::new(ch),
            finalist: FdgFinalist::new(srate, ch),
            p_tilt: TiltParams {
                bypass: 1,
                gain: FDG_TILT_DEFAULT_GAIN,
                pivot: FDG_TILT_DEFAULT_PIVOT,
                slope: FDG_TILT_DEFAULT_SLOPE,
            },
            p_nice: NiceParams {
                bypass: 1,
                bass_amount: FDG_NICE_DEFAULT_BASS,
                hi_amount: FDG_NICE_DEFAULT_HI,
            },
            p_spread: SpreadParams {
                bypass: 1,
                expander: 0.5,
                inverse: 0,
                center_amount: 1.0,
            },
            p_crush: CrushParams {
                bypass: 1,
                bits: FDG_CRUSH_DEFAULT_BITS,
                rate: FDG_CRUSH_DEFAULT_RATE,
                mix: 0.0,
            },
            p_finalist: FinalistParams {
                bypass: 1,
                input_db: 0.0,
                drive: FDG_FINALIST_DEFAULT_DRIVE,
                ceiling_db: FDG_FINALIST_DEFAULT_CEILING_DB,
                release_ms: FDG_FINALIST_DEFAULT_RELEASE_MS,
            },
        };
        inst.apply_params();
        inst
    }

    /// 保持しているパラメータを各段へ流し直す。
    ///
    /// 既存の `fdg_*_set_params` を経由するのは、clamp と dirty 判定を
    /// 単体呼び出し（= fb2k 版）と 1 文字も違わせないため。ビット一致の前提。
    fn apply_params(&mut self) {
        unsafe {
            let t = self.p_tilt;
            fdg_tilt_set_params(&mut self.tilt, t.bypass, t.gain, t.pivot, t.slope);
            let n = self.p_nice;
            fdg_nice_set_params(&mut self.nice, n.bypass, n.bass_amount, n.hi_amount);
            let s = self.p_spread;
            fdg_spread_set_params(
                &mut self.spread,
                s.bypass,
                s.expander,
                s.inverse,
                s.center_amount,
            );
            let c = self.p_crush;
            fdg_crush_set_params(&mut self.crush, c.bypass, c.bits, c.rate, c.mix);
            let f = self.p_finalist;
            fdg_finalist_set_params(
                &mut self.finalist,
                f.bypass,
                f.input_db,
                f.drive,
                f.ceiling_db,
                f.release_ms,
            );
        }
    }

    /// 各段の内部状態だけを落とす（レートとチャンネル数は変えない）。シークと曲頭。
    fn reset_stages(&mut self) {
        let srate = self.srate;
        unsafe {
            fdg_tilt_reset(&mut self.tilt, srate);
            fdg_nice_reset(&mut self.nice, srate);
            fdg_spread_reset(&mut self.spread, srate);
            fdg_crush_reset(&mut self.crush);
            fdg_finalist_reset(&mut self.finalist, srate);
        }
    }

    /// I7 の手順。**非 RT スレッドから呼ぶ**（チャンネル数が変わると `Vec` を作り直す）。
    /// 戻り値は再取得したレイテンシ（F16）。
    fn prepare(&mut self, srate: f32, channel_count: usize) -> u32 {
        let srate = if srate.is_finite() && srate > 0.0 {
            srate
        } else {
            self.srate
        };
        let ch = channel_count.max(1);
        if ch == self.ch {
            // I7: チャンネル数が同じなら各段に reset(srate) を呼ぶ。
            // レートが 0.5 Hz 以上変われば、各段の reset が内部で係数を作り直す。
            self.srate = srate;
            self.reset_stages();
        } else {
            // I7: チャンネル数が変わるなら作り直す（**I6 の順序で**）。
            // `free` → `new` は、所有が `Box` ではなくこの構造体なので
            // フィールドの置き換えで表す。
            self.srate = srate;
            self.ch = ch;
            self.tilt = FdgTilt::new(srate, ch);
            self.nice = FdgNiceBulk::new(srate, ch);
            self.spread = FdgSpread::new(srate);
            self.crush = FdgCrush::new(ch);
            self.finalist = FdgFinalist::new(srate, ch);
            // 作り直しでパラメータを取り落とすと、曲の切り替えで設定が黙って
            // 既定に戻る。L1 の責任 #8 はここ。
            self.apply_params();
        }
        self.latency()
    }

    /// F16: finalist から**再取得**する。chain 側に写し取って持たない。
    fn latency(&mut self) -> u32 {
        unsafe { fdg_finalist_latency(&mut self.finalist) }
    }

    fn render(&mut self, out: &mut [f32], frames: usize) {
        let ch = self.ch;
        // I6 の固定順: tilt → nice-bulk → spread → crush → finalist。
        // **crush を finalist の後ろに置かない**（実測で入力ピーク 1.0 が最大 2.0 まで
        // 膨らみ、天井が天井でなくなる。I6 の禁止条項）。
        self.tilt.process_interleaved(out, ch, frames);
        self.nice.process_interleaved(out, ch, frames);
        // spread はステレオ幅の装置なので 2ch のときだけ効く。モノに「幅」は無く、
        // 3ch 以上ではどのペアに当てるかが決まらない。別物を黙って通すより素通し。
        if ch == 2 {
            self.spread.process_interleaved_stereo(out, frames);
        }
        self.crush.process_interleaved(out, ch, frames);
        self.finalist.process_interleaved(out, ch, frames);
    }
}

#[no_mangle]
pub extern "C" fn fdg_chain_new(srate: f32, channel_count: u32) -> *mut FdgChain {
    match catch_unwind(|| Box::into_raw(Box::new(FdgChain::new(srate, channel_count as usize)))) {
        Ok(p) => p,
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn fdg_chain_free(p: *mut FdgChain) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        drop(Box::from_raw(p));
    }));
}

/// レート / チャンネル数を反映し、レイテンシを返す（I7・F16）。
/// **オーディオスレッドの外で呼ぶこと。** 曲の切り替えで 1 回。
#[no_mangle]
pub unsafe extern "C" fn fdg_chain_prepare(
    p: *mut FdgChain,
    srate: f32,
    channel_count: u32,
) -> u32 {
    if p.is_null() {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| {
        (*p).prepare(srate, channel_count as usize)
    }))
    .unwrap_or(0)
}

/// 直列全体の群遅延（サンプル）。finalist から再取得した値（F16）。
#[no_mangle]
pub unsafe extern "C" fn fdg_chain_latency(p: *mut FdgChain) -> u32 {
    if p.is_null() {
        return 0;
    }
    catch_unwind(AssertUnwindSafe(|| (*p).latency())).unwrap_or(0)
}

/// 各段の内部状態を落とす。レートとチャンネル数は変えないので、
/// レイテンシも変わらない（変える場合は `fdg_chain_prepare`）。
#[no_mangle]
pub unsafe extern "C" fn fdg_chain_reset(p: *mut FdgChain) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        (*p).reset_stages();
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_chain_set_tilt_params(
    p: *mut FdgChain,
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
        inst.p_tilt = TiltParams {
            bypass,
            gain,
            pivot,
            slope,
        };
        fdg_tilt_set_params(&mut inst.tilt, bypass, gain, pivot, slope);
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_chain_set_nice_params(
    p: *mut FdgChain,
    bypass: i32,
    bass_amount: f32,
    hi_amount: f32,
) {
    if p.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let inst = &mut *p;
        inst.p_nice = NiceParams {
            bypass,
            bass_amount,
            hi_amount,
        };
        fdg_nice_set_params(&mut inst.nice, bypass, bass_amount, hi_amount);
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_chain_set_spread_params(
    p: *mut FdgChain,
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
        inst.p_spread = SpreadParams {
            bypass,
            expander,
            inverse,
            center_amount,
        };
        fdg_spread_set_params(&mut inst.spread, bypass, expander, inverse, center_amount);
    }));
}

/// `rate` は既存の `fdg_crush_set_params` と同じく**実係数**（1.0..=32.0）。
/// `2^p` の変換は呼び出し側（`plans/crush_rate_fb2k_v01.md` §2-1）。
#[no_mangle]
pub unsafe extern "C" fn fdg_chain_set_crush_params(
    p: *mut FdgChain,
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
        inst.p_crush = CrushParams {
            bypass,
            bits,
            rate,
            mix,
        };
        fdg_crush_set_params(&mut inst.crush, bypass, bits, rate, mix);
    }));
}

#[no_mangle]
pub unsafe extern "C" fn fdg_chain_set_finalist_params(
    p: *mut FdgChain,
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
        inst.p_finalist = FinalistParams {
            bypass,
            input_db,
            drive,
            ceiling_db,
            release_ms,
        };
        fdg_finalist_set_params(
            &mut inst.finalist,
            bypass,
            input_db,
            drive,
            ceiling_db,
            release_ms,
        );
    }));
}

/// pull 形（§3-2）。iOS は `AVAudioSourceNode` の render ブロックから、
/// Android は `AudioTrack` の blocking `write()` ループから呼ぶ。
///
/// **L1 の現在の約束**: `out` には呼び出し側が入力 PCM を**先に詰めてある**。
/// chain はそれをインプレースで処理する。長さは
/// `frames * channel_count`（`fdg_chain_prepare` に渡したチャンネル数）。
/// インターリーブ f32。既存の `fdg_*_process` と同じ約束。
///
/// 署名を `process(in, out, frames)` ではなく `render(out, frames)` にしてあるのは、
/// L2（リングバッファを Rust へ上げる。**未裁定・Step 5 の直前**）を採ったときに
/// 「Rust が `out` を自分で埋める」へ意味だけ移せるようにするため。
///
/// **この関数はアロケーションしない**（I4）。レートとチャンネル数が
/// `fdg_chain_prepare` と一致している限り、各段の `ensure_layout` は何もしない。
#[no_mangle]
pub unsafe extern "C" fn fdg_chain_render(p: *mut FdgChain, out: *mut f32, frames: usize) {
    if p.is_null() || out.is_null() || frames == 0 {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        enable_ftz_daz();
        let inst = &mut *p;
        let slice = std::slice::from_raw_parts_mut(out, frames * inst.ch);
        inst.render(slice, frames);
    }));
}

#[cfg(test)]
mod chain_tests {
    //! L1 の判定器（`plans/ios_player_v02.md` §10-B-2）。
    //!
    //! 中心は **ビット一致**: `fdg_chain_render` の出力が、個別の `fdg_*_process` を
    //! I6 の順に呼んだ結果と f32 の等値で一致する。許容誤差を置かない。
    //! `cargo check` だけでは通ってしまう「もっともらしい破壊」
    //! （段の取り違え・順序の入れ替え・spread の planar/interleaved のずれ）を
    //! これが素通りさせない。

    use super::*;

    const SR: f32 = 48_000.0;

    // 全段を**効かせた**設定。既定値から離してある。bypass のまま測ると
    // 素通りして順序の誤りを見逃すため。
    const TILT: (i32, f32, f32, f32) = (0, 4.0, 900.0, 0.55);
    const NICE: (i32, f32, f32) = (0, 0.45, 0.28);
    const SPREAD: (i32, f32, i32, f32) = (0, 0.82, 0, 0.6);
    const CRUSH: (i32, f32, f32, f32) = (0, 6.0, 3.0, 0.7);
    const FINALIST: (i32, f32, f32, f32, f32) = (0, 3.0, 0.5, -1.0, 60.0);

    /// 非自明な素材。低域・中域・高域・過渡・L/R 差を全部入れる
    /// （どの段を抜いても出力が変わる形にするため）。
    fn make_input(frames: usize, ch: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; frames * ch];
        for f in 0..frames {
            let t = f as f32 / SR;
            for c in 0..ch {
                let detune = 1.0 + 0.013 * c as f32;
                let mut x = 0.35 * (2.0 * std::f32::consts::PI * 60.0 * detune * t).sin()
                    + 0.22 * (2.0 * std::f32::consts::PI * 997.0 * t).sin()
                    + 0.18 * (2.0 * std::f32::consts::PI * 6300.0 * detune * t).sin();
                if f % 877 == 0 {
                    x += 0.9; // 過渡。finalist のリリースと crush の S&H を動かす
                }
                v[f * ch + c] = x;
            }
        }
        v
    }

    /// 基準側: 個別の `fdg_*_process` を **I6 の順**に呼ぶ。
    /// spread だけは planar L/R の API なので、外して戻す。
    fn reference_render(srate: f32, ch: usize, frames: usize, data: &mut [f32]) {
        unsafe {
            let tilt = fdg_tilt_new(srate, ch as u32);
            let nice = fdg_nice_new(srate, ch as u32);
            let spread = fdg_spread_new(srate);
            let crush = fdg_crush_new(ch as u32);
            let finalist = fdg_finalist_new(srate, ch as u32);
            assert!(!tilt.is_null() && !nice.is_null() && !spread.is_null());
            assert!(!crush.is_null() && !finalist.is_null());

            fdg_tilt_set_params(tilt, TILT.0, TILT.1, TILT.2, TILT.3);
            fdg_nice_set_params(nice, NICE.0, NICE.1, NICE.2);
            fdg_spread_set_params(spread, SPREAD.0, SPREAD.1, SPREAD.2, SPREAD.3);
            fdg_crush_set_params(crush, CRUSH.0, CRUSH.1, CRUSH.2, CRUSH.3);
            fdg_finalist_set_params(
                finalist,
                FINALIST.0,
                FINALIST.1,
                FINALIST.2,
                FINALIST.3,
                FINALIST.4,
            );

            let n = ch as u32;
            fdg_tilt_process(tilt, data.as_mut_ptr(), n, frames);
            fdg_nice_process(nice, data.as_mut_ptr(), n, frames);
            if ch == 2 {
                let mut l: Vec<f32> = (0..frames).map(|f| data[f * 2]).collect();
                let mut r: Vec<f32> = (0..frames).map(|f| data[f * 2 + 1]).collect();
                fdg_spread_process(spread, l.as_mut_ptr(), r.as_mut_ptr(), frames);
                for f in 0..frames {
                    data[f * 2] = l[f];
                    data[f * 2 + 1] = r[f];
                }
            }
            fdg_crush_process(crush, data.as_mut_ptr(), n, frames);
            fdg_finalist_process(finalist, data.as_mut_ptr(), n, frames);

            fdg_tilt_free(tilt);
            fdg_nice_free(nice);
            fdg_spread_free(spread);
            fdg_crush_free(crush);
            fdg_finalist_free(finalist);
        }
    }

    unsafe fn set_all(c: *mut FdgChain) {
        fdg_chain_set_tilt_params(c, TILT.0, TILT.1, TILT.2, TILT.3);
        fdg_chain_set_nice_params(c, NICE.0, NICE.1, NICE.2);
        fdg_chain_set_spread_params(c, SPREAD.0, SPREAD.1, SPREAD.2, SPREAD.3);
        fdg_chain_set_crush_params(c, CRUSH.0, CRUSH.1, CRUSH.2, CRUSH.3);
        fdg_chain_set_finalist_params(
            c,
            FINALIST.0,
            FINALIST.1,
            FINALIST.2,
            FINALIST.3,
            FINALIST.4,
        );
    }

    fn assert_bit_identical(got: &[f32], want: &[f32], label: &str) {
        assert_eq!(got.len(), want.len(), "{label}: 長さが違う");
        for i in 0..got.len() {
            assert_eq!(
                got[i].to_bits(),
                want[i].to_bits(),
                "{label}: i={i} got={} want={}",
                got[i],
                want[i]
            );
        }
    }

    /// **L1 の判定器。** chain の出力が個別呼び出し（I6 の順）とビット一致する。
    #[test]
    fn chain_matches_individual_calls() {
        let (ch, frames) = (2usize, 4096usize);
        let input = make_input(frames, ch);

        let mut via_chain = input.clone();
        unsafe {
            let c = fdg_chain_new(SR, ch as u32);
            assert!(!c.is_null());
            set_all(c);
            fdg_chain_render(c, via_chain.as_mut_ptr(), frames);
            fdg_chain_free(c);
        }

        let mut via_parts = input.clone();
        reference_render(SR, ch, frames, &mut via_parts);

        // 素通りしていないこと（恒等を一致と誤認しないため）。
        assert!(
            via_parts.iter().zip(input.iter()).any(|(a, b)| a != b),
            "基準側が入力と同一。設定が効いていない"
        );
        assert_bit_identical(&via_chain, &via_parts, "stereo");
    }

    /// モノ。spread は 2ch のときだけ効く（`FdgChain::render` の約束）ので、
    /// 基準側も spread を通さない。
    #[test]
    fn chain_matches_individual_calls_mono() {
        let (ch, frames) = (1usize, 2048usize);
        let input = make_input(frames, ch);

        let mut via_chain = input.clone();
        unsafe {
            let c = fdg_chain_new(SR, ch as u32);
            set_all(c);
            fdg_chain_render(c, via_chain.as_mut_ptr(), frames);
            fdg_chain_free(c);
        }

        let mut via_parts = input.clone();
        reference_render(SR, ch, frames, &mut via_parts);
        assert_bit_identical(&via_chain, &via_parts, "mono");
    }

    /// I7 の作り直し経路。チャンネル数を変えた後も、
    /// ①各段が新しいチャンネル数で作り直され ②**パラメータが流し直される**。
    /// ②が抜けると設定が黙って既定へ戻る（コンパイルは通る）。
    #[test]
    fn chain_keeps_params_across_channel_change() {
        let frames = 2048usize;
        let input = make_input(frames, 1);

        let mut via_chain = input.clone();
        unsafe {
            let c = fdg_chain_new(SR, 2);
            set_all(c);
            // 2ch → 1ch。I7 では free → new の作り直しになる経路。
            fdg_chain_prepare(c, SR, 1);
            fdg_chain_render(c, via_chain.as_mut_ptr(), frames);
            fdg_chain_free(c);
        }

        let mut via_parts = input.clone();
        reference_render(SR, 1, frames, &mut via_parts);
        assert_bit_identical(&via_chain, &via_parts, "after channel change");
    }

    /// 受け入れ基準 3 の「全段無効で入力とビット一致」。
    ///
    /// finalist は bypass でも**遅延した dry** を返す（天井保証が bypass 状態に
    /// 依存しないようにするため。`crates/finalist/src/dsp.rs:203-205`）。
    /// よって恒等はレイテンシ分ずらして見る。ずらす量の出所が
    /// `fdg_chain_latency` であることが、プラットフォーム側の補償の根拠。
    #[test]
    fn chain_all_stages_inert_is_identity_after_latency() {
        let (ch, frames) = (2usize, 8192usize);
        let input = make_input(frames, ch);
        let mut out = input.clone();

        let d = unsafe {
            let c = fdg_chain_new(SR, ch as u32);
            // 基準の文言どおり: crush は mix=0（bypass ではない）、他は bypass。
            fdg_chain_set_tilt_params(c, 1, TILT.1, TILT.2, TILT.3);
            fdg_chain_set_nice_params(c, 1, NICE.1, NICE.2);
            fdg_chain_set_spread_params(c, 1, SPREAD.1, SPREAD.2, SPREAD.3);
            fdg_chain_set_crush_params(c, 0, CRUSH.1, CRUSH.2, 0.0);
            fdg_chain_set_finalist_params(c, 1, FINALIST.1, FINALIST.2, FINALIST.3, FINALIST.4);
            let d = fdg_chain_latency(c) as usize;
            fdg_chain_render(c, out.as_mut_ptr(), frames);
            fdg_chain_free(c);
            d
        };

        assert!(d > 0 && d < frames, "latency={d} frames={frames}");
        for i in 0..d * ch {
            assert_eq!(out[i].to_bits(), 0.0f32.to_bits(), "先頭が無音でない i={i}");
        }
        for i in 0..(frames - d) * ch {
            assert_eq!(
                out[d * ch + i].to_bits(),
                input[i].to_bits(),
                "恒等が破れた i={i} got={} want={}",
                out[d * ch + i],
                input[i]
            );
        }
    }

    /// F16・I7: `fdg_chain_latency` は finalist と一致し、`fdg_chain_prepare` で
    /// レートを変えた後は**再取得で変わる**。
    #[test]
    fn chain_latency_matches_finalist_and_follows_rate() {
        unsafe {
            let c = fdg_chain_new(SR, 2);
            let f48 = fdg_finalist_new(SR, 2);
            let d48 = fdg_finalist_latency(f48);
            fdg_finalist_free(f48);
            assert_eq!(fdg_chain_latency(c), d48, "48k で finalist と一致しない");

            let returned = fdg_chain_prepare(c, 44_100.0, 2);
            let f441 = fdg_finalist_new(44_100.0, 2);
            let d441 = fdg_finalist_latency(f441);
            fdg_finalist_free(f441);

            assert_eq!(returned, d441, "prepare の戻り値が finalist と一致しない");
            assert_eq!(fdg_chain_latency(c), d441, "再取得で一致しない");
            assert_ne!(d48, d441, "レートを変えてもレイテンシが変わっていない");
            fdg_chain_free(c);
        }
    }

    /// null と frames=0 で落ちない（既存 26 本と同じ約束）。
    #[test]
    fn chain_null_and_zero_frames_are_safe() {
        unsafe {
            assert_eq!(fdg_chain_latency(std::ptr::null_mut()), 0);
            assert_eq!(fdg_chain_prepare(std::ptr::null_mut(), SR, 2), 0);
            fdg_chain_reset(std::ptr::null_mut());
            fdg_chain_free(std::ptr::null_mut());
            fdg_chain_render(std::ptr::null_mut(), std::ptr::null_mut(), 64);

            let c = fdg_chain_new(SR, 2);
            let mut buf = vec![0.5f32; 8];
            fdg_chain_render(c, buf.as_mut_ptr(), 0);
            assert!(buf.iter().all(|x| *x == 0.5), "frames=0 で書き換わった");
            fdg_chain_reset(c);
            fdg_chain_free(c);
        }
    }
}
