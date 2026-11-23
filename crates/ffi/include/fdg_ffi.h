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

#ifdef __cplusplus
}
#endif
