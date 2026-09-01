#include "stdafx.h"
#include "fdg_ffi.h"
#include "resource.h"

#include <helpers/DarkMode.h>

#include <cmath>
#include <vector>

namespace {

// 9d8c3830-ef48-43eb-9306-aee3d709ab4c
static constexpr GUID k_crush_guid = {
	0x9d8c3830,
	0xef48,
	0x43eb,
	{ 0x93, 0x06, 0xae, 0xe3, 0xd7, 0x09, 0xab, 0x4c }
};

// Listening defaults (VST: bits=8 rate=1 mix=1.0).
// These are deliberately different from the VST defaults so that the effect is
// audible right after inserting the DSP. Do not "fix" them to match the VST.
static constexpr float k_def_bits = 10.0f;
static constexpr float k_def_rate = 2.0f;
static constexpr float k_def_mix = 0.40f;

// Preset format version.
//   1: rate stored as uint32_t (1..32), integer only.
//   2: rate stored as float (1.0..32.0). The VST side made rate continuous
//      (plans/crush_rate_step_v02.md); this port follows.
// Version 1 presets are migrated on load, never dropped.
static constexpr uint8_t k_version = 2;

struct crush_params_t {
	uint8_t version = k_version;
	uint8_t bypass = 0;
	float bits = k_def_bits;
	// Actual S&H divisor (1.0..32.0), not the log2 value. The dialog holds the
	// slider in log2 space; the conversion happens there.
	float rate = k_def_rate;
	float mix = k_def_mix;
};

static void make_preset(const crush_params_t& p, dsp_preset& out) {
	dsp_preset_builder builder;
	builder << p.version << p.bypass << p.bits << p.rate << p.mix;
	builder.finish(k_crush_guid, out);
}

static crush_params_t parse_preset(const dsp_preset& in) {
	crush_params_t p;
	try {
		dsp_preset_parser parser(in);
		parser >> p.version >> p.bypass >> p.bits;
		if (p.version == 1) {
			// Migrate: rate used to be uint32_t in 1..32. The numeric range is
			// unchanged, so the old value carries over as-is.
			uint32_t legacy_rate = 0;
			parser >> legacy_rate >> p.mix;
			p.rate = static_cast<float>(legacy_rate);
			p.version = k_version;
		} else if (p.version == k_version) {
			parser >> p.rate >> p.mix;
		} else {
			return crush_params_t{};
		}
	} catch (exception_io_data const&) {
		return crush_params_t{};
	}
	return p;
}

static void apply_params(FdgCrush* inst, const crush_params_t& p) {
	if (!inst) {
		return;
	}
	fdg_crush_set_params(inst, p.bypass ? 1 : 0, p.bits, p.rate, p.mix);
}

static void RunCrushConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback);

} // namespace

class dsp_fdg_crush : public dsp_impl_base {
public:
	dsp_fdg_crush(dsp_preset const& in) : m_params(parse_preset(in)) {}

	~dsp_fdg_crush() {
		if (m_inst) {
			fdg_crush_free(m_inst);
			m_inst = nullptr;
		}
	}

	static GUID g_get_guid() { return k_crush_guid; }
	static void g_get_name(pfc::string_base& p_out) { p_out = "fandango crush"; }
	static bool g_get_default_preset(dsp_preset& p_out) {
		make_preset(crush_params_t{}, p_out);
		return true;
	}
	static bool g_have_config_popup() { return true; }
	static void g_show_config_popup(
		const dsp_preset& p_data,
		HWND p_parent,
		dsp_preset_edit_callback& p_callback) {
		RunCrushConfigPopup(p_data, p_parent, p_callback);
	}

	bool on_chunk(audio_chunk* chunk, abort_callback&) {
		if (!chunk) {
			return true;
		}
		const unsigned ch = chunk->get_channel_count();
		if (ch == 0) {
			return true;
		}
		ensure_instance(ch);
		const t_size frames = chunk->get_sample_count();
		if (frames == 0 || !m_inst) {
			return true;
		}
		audio_sample* data = chunk->get_data();
		const t_size total = frames * ch;
		m_scratch.resize(total);
		for (t_size i = 0; i < total; ++i) {
			m_scratch[i] = static_cast<float>(data[i]);
		}
		fdg_crush_process(m_inst, m_scratch.data(), ch, frames);
		for (t_size i = 0; i < total; ++i) {
			data[i] = static_cast<audio_sample>(m_scratch[i]);
		}
		return true;
	}

	void on_endofplayback(abort_callback&) {}
	void on_endoftrack(abort_callback&) {}
	void flush() {
		if (m_inst) {
			fdg_crush_reset(m_inst);
		}
	}
	double get_latency() { return 0; }
	bool need_track_change_mark() { return false; }

private:
	void ensure_instance(unsigned ch) {
		if (!m_inst) {
			m_inst = fdg_crush_new(ch);
			m_ch = ch;
			apply_params(m_inst, m_params);
			return;
		}
		if (ch != m_ch) {
			fdg_crush_free(m_inst);
			m_inst = fdg_crush_new(ch);
			m_ch = ch;
			apply_params(m_inst, m_params);
		}
	}

	FdgCrush* m_inst = nullptr;
	unsigned m_ch = 0;
	std::vector<float> m_scratch;
	crush_params_t m_params;
};

static dsp_factory_t<dsp_fdg_crush> g_fdg_crush_factory;

namespace {

class CCrushDSPPopup : public CDialogImpl<CCrushDSPPopup> {
public:
	CCrushDSPPopup(const dsp_preset& initData, dsp_preset_edit_callback& callback)
		: m_initData(initData), m_callback(callback) {}

	enum { IDD = IDD_DSP_CRUSH };
	// bits: 10..160 => 1.0..16.0 ; mix: 0..100 => 0.0..1.0
	// rate: 0..500 => log2(rate) 0.00..5.00 => rate 1.0..32.0
	// The slider lives in log2 space so that equal travel gives an equal ratio
	// change in the effective rate (plans/crush_rate_fb2k_v01.md 2-2).
	// CTrackBarCtrl is integer-only, so we use hundredths of an octave, the
	// same trick the bits slider uses with tenths.
	enum { BitsMin = 10, BitsMax = 160, RateOctMax = 500, MixMax = 100 };

	BEGIN_MSG_MAP_EX(CCrushDSPPopup)
		MSG_WM_INITDIALOG(OnInitDialog)
		COMMAND_HANDLER_EX(IDOK, BN_CLICKED, OnButton)
		COMMAND_HANDLER_EX(IDCANCEL, BN_CLICKED, OnButton)
		COMMAND_HANDLER_EX(IDC_RESET, BN_CLICKED, OnReset)
		COMMAND_HANDLER_EX(IDC_CHECK_BYPASS, BN_CLICKED, OnCheck)
		MSG_WM_HSCROLL(OnHScroll)
	END_MSG_MAP()

private:
	BOOL OnInitDialog(CWindow, LPARAM) {
		m_dark.AddDialogWithControls(m_hWnd);
		m_bits = GetDlgItem(IDC_SLIDER_BITS);
		m_rate = GetDlgItem(IDC_SLIDER_RATE);
		m_mix = GetDlgItem(IDC_SLIDER_MIX);
		m_bits.SetRange(BitsMin, BitsMax);
		m_rate.SetRange(0, RateOctMax);
		m_mix.SetRange(0, MixMax);
		apply_to_ui(parse_preset(m_initData));
		return TRUE;
	}

	void OnButton(UINT, int id, CWindow) { EndDialog(id); }
	void OnReset(UINT, int, CWindow) {
		apply_to_ui(crush_params_t{});
		push_live();
	}
	void OnCheck(UINT, int, CWindow) { push_live(); }
	void OnHScroll(UINT, UINT, CScrollBar) { push_live(); }

	void apply_to_ui(const crush_params_t& p) {
		const int bits_pos = pfc::clip_t<int>(
			pfc::rint32(p.bits * 10.f), BitsMin, BitsMax);
		m_bits.SetPos(bits_pos);
		const float rate_safe = p.rate < 1.0f ? 1.0f : p.rate;
		const int rate_pos = pfc::clip_t<int>(
			pfc::rint32(std::log2(rate_safe) * 100.f), 0, RateOctMax);
		m_rate.SetPos(rate_pos);
		// Remember what the slider position means, so that read_ui() can hand the
		// exact value back while the user has not touched this slider. The tick
		// grid is exponential, so a value like 31.0 does not sit on it: without
		// this, migrating a version 1 preset and then nudging *any other* slider
		// would silently round rate 31 -> 30.91.
		m_rate_tick = rate_pos;
		m_rate_value = rate_safe;
		m_mix.SetPos(pfc::clip_t<int>(pfc::rint32(p.mix * 100.f), 0, MixMax));
		CheckDlgButton(IDC_CHECK_BYPASS, p.bypass ? BST_CHECKED : BST_UNCHECKED);
		RefreshLabels(p);
	}

	crush_params_t read_ui() {
		crush_params_t p;
		p.bits = static_cast<float>(m_bits.GetPos()) / 10.f;
		// Tick 0 gives exactly 1.0 (the identity point): powf(2, 0) == 1.0f.
		// Only recompute when the slider actually moved (see apply_to_ui).
		const int rate_tick = m_rate.GetPos();
		if (rate_tick == m_rate_tick) {
			p.rate = m_rate_value;
		} else {
			p.rate = std::pow(2.0f, static_cast<float>(rate_tick) / 100.f);
			m_rate_tick = rate_tick;
			m_rate_value = p.rate;
		}
		p.mix = static_cast<float>(m_mix.GetPos()) / 100.f;
		p.bypass = IsDlgButtonChecked(IDC_CHECK_BYPASS) == BST_CHECKED ? 1 : 0;
		return p;
	}

	void push_live() {
		const crush_params_t p = read_ui();
		dsp_preset_impl preset;
		make_preset(p, preset);
		m_callback.on_preset_changed(preset);
		RefreshLabels(p);
	}

	void RefreshLabels(const crush_params_t& p) {
		pfc::string_formatter msg;
		msg << pfc::format_float(p.bits, 0, 1);
		::uSetDlgItemText(*this, IDC_LABEL_BITS, msg);
		msg.reset();
		msg << pfc::format_float(p.rate, 0, 2);
		::uSetDlgItemText(*this, IDC_LABEL_RATE, msg);
		msg.reset();
		msg << pfc::format_int(pfc::rint32(p.mix * 100.f)) << "%";
		::uSetDlgItemText(*this, IDC_LABEL_MIX, msg);
	}

	const dsp_preset& m_initData;
	dsp_preset_edit_callback& m_callback;
	CTrackBarCtrl m_bits, m_rate, m_mix;
	// Last slider tick we wrote or read, and the rate it stands for. Guards the
	// exponential tick grid from rounding a value that is not on it.
	int m_rate_tick = -1;
	float m_rate_value = k_def_rate;
	fb2k::CDarkModeHooks m_dark;
};

static void RunCrushConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback) {
	CCrushDSPPopup popup(p_data, p_callback);
	if (popup.DoModal(p_parent) != IDOK) {
		p_callback.on_preset_changed(p_data);
	}
}

} // namespace
