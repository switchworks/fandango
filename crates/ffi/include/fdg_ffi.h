/* C view of crates/ffi/src/lib.rs. Keep the two in sync by hand.
 *
 * A mismatch between these declarations and the Rust #[no_mangle] signatures
 * is NOT caught at link time (the C ABI does not carry types), so it breaks
 * silently. That is why this header lives next to the implementation rather
 * than inside any one consumer's folder: it was under
 * fb2k/foo_dsp_fandango/ until 2026-11-24, which read as if the foobar2000
 * component owned it. Consumers copy it from here.
 */
#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FdgSpread FdgSpread;

FdgSpread* fdg_spread_new(float srate);
void fdg_spread_free(FdgSpread* p);
void fdg_spread_set_params(
	FdgSpread* p,
	int32_t bypass,
	float expander,
	int32_t inverse,
	float center_amount);
void fdg_spread_reset(FdgSpread* p, float srate);
void fdg_spread_process(FdgSpread* p, float* left, float* right, size_t frames);

typedef struct FdgNiceBulk FdgNiceBulk;

FdgNiceBulk* fdg_nice_new(float srate, uint32_t channel_count);
void fdg_nice_free(FdgNiceBulk* p);
void fdg_nice_set_params(
	FdgNiceBulk* p,
	int32_t bypass,
	float bass_amount,
	float hi_amount);
void fdg_nice_reset(FdgNiceBulk* p, float srate);
void fdg_nice_process(
	FdgNiceBulk* p,
	float* data,
	uint32_t channel_count,
	size_t frames);

typedef struct FdgCrush FdgCrush;
FdgCrush* fdg_crush_new(uint32_t channel_count);
void fdg_crush_free(FdgCrush* p);
/* rate is the actual S&H divisor (1.0..32.0), not the log2 value.
   The caller does the 2^p conversion. See plans/crush_rate_fb2k_v01.md. */
void fdg_crush_set_params(
	FdgCrush* p,
	int32_t bypass,
	float bits,
	float rate,
	float mix);
void fdg_crush_reset(FdgCrush* p);
void fdg_crush_process(
	FdgCrush* p,
	float* data,
	uint32_t channel_count,
	size_t frames);

typedef struct FdgTilt FdgTilt;
FdgTilt* fdg_tilt_new(float srate, uint32_t channel_count);
void fdg_tilt_free(FdgTilt* p);
void fdg_tilt_set_params(
	FdgTilt* p,
	int32_t bypass,
	float gain,
	float pivot,
	float slope);
void fdg_tilt_reset(FdgTilt* p, float srate);
void fdg_tilt_process(
	FdgTilt* p,
	float* data,
	uint32_t channel_count,
	size_t frames);

typedef struct FdgFinalist FdgFinalist;
FdgFinalist* fdg_finalist_new(float srate, uint32_t channel_count);
void fdg_finalist_free(FdgFinalist* p);
void fdg_finalist_set_params(
	FdgFinalist* p,
	int32_t bypass,
	float input_db,
	float drive,
	float ceiling_db,
	float release_ms);
void fdg_finalist_reset(FdgFinalist* p, float srate);
uint32_t fdg_finalist_latency(FdgFinalist* p);
void fdg_finalist_process(
	FdgFinalist* p,
	float* data,
	uint32_t channel_count,
	size_t frames);

/* -------------------------------------------------------------------------
   chain -- the L1 FFI boundary (plans/ios_player_v02.md 3-2 / I6 / I7 / I11).
   Comments in this file stay ASCII: MSVC without /utf-8 reads the source as
   the system codepage, and a CP932 lead byte can eat the closing delimiter.

   This is an ADDITION to the 26 functions above; not one of those
   declarations changed (the shipped foobar2000 component calls them on real
   hardware). The fb2k component does not use the chain.

   Fixed serial order: tilt -> nice-bulk -> spread -> crush -> finalist.
   No reordering is offered. Putting crush after finalist breaks the ceiling:
   mid-tread quantisation can give |q(x)| > |x| (measured up to +6 dB).

   The consumer owes exactly two things:
     - call fdg_chain_prepare once per track change, OFF the audio thread
     - use the returned latency for display and for the tail flush
   The order, the rebuild procedure and the latency re-fetch all live on the
   Rust side. Do not copy them here: keeping that promise in one place is the
   whole point of L1.
   ------------------------------------------------------------------------- */

typedef struct FdgChain FdgChain;

/* A fresh chain has every stage bypassed (crush at mix=0, an exact identity).
   Call the five setters below before rendering. The listening defaults are
   still undecided, so this constructor deliberately picks none. */
FdgChain* fdg_chain_new(float srate, uint32_t channel_count);
void fdg_chain_free(FdgChain* p);

/* Apply a new rate / channel count and return the latency in samples.
   CALL THIS OFF THE AUDIO THREAD: a channel-count change allocates. */
uint32_t fdg_chain_prepare(FdgChain* p, float srate, uint32_t channel_count);

/* Group delay of the whole serial chain, in samples (re-fetched from
   finalist, never cached by the chain). */
uint32_t fdg_chain_latency(FdgChain* p);

/* Drop every stage's internal state. Rate and channel count are untouched,
   so the latency does not change either (use fdg_chain_prepare for that). */
void fdg_chain_reset(FdgChain* p);

void fdg_chain_set_tilt_params(
	FdgChain* p,
	int32_t bypass,
	float gain,
	float pivot,
	float slope);
void fdg_chain_set_nice_params(
	FdgChain* p,
	int32_t bypass,
	float bass_amount,
	float hi_amount);
/* spread is a stereo-width device, so it only acts when channel_count == 2. */
void fdg_chain_set_spread_params(
	FdgChain* p,
	int32_t bypass,
	float expander,
	int32_t inverse,
	float center_amount);
/* rate is the actual S&H divisor (1.0..32.0), as in fdg_crush_set_params.
   The caller does the 2^p conversion. */
void fdg_chain_set_crush_params(
	FdgChain* p,
	int32_t bypass,
	float bits,
	float rate,
	float mix);
void fdg_chain_set_finalist_params(
	FdgChain* p,
	int32_t bypass,
	float input_db,
	float drive,
	float ceiling_db,
	float release_ms);

/* Pull form: iOS calls it from the AVAudioSourceNode render block, Android
   from the AudioTrack blocking write() loop.

   CONTRACT AT L1: the caller has ALREADY filled `out` with the input PCM;
   the chain processes it in place. Length is frames * channel_count (the
   count passed to fdg_chain_prepare), interleaved f32 -- the same contract
   as the fdg_*_process functions above.
   The signature is render(out, frames) rather than process(in, out, frames)
   so that adopting L2 (moving the ring buffer into Rust; still undecided)
   only changes the meaning to "Rust fills out", not the signature.

   This function does not allocate. */
void fdg_chain_render(FdgChain* p, float* out, size_t frames);

#ifdef __cplusplus
}
#endif
