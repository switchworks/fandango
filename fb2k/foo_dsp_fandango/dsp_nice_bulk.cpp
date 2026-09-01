#include "stdafx.h"
#include "fdg_ffi.h"
#include "resource.h"

#include <helpers/DarkMode.h>

#include <vector>

namespace {

// 8a98be19-1b7b-4300-b705-28ef0e2d7e72
static constexpr GUID k_nice_guid = {
	0x8a98be19,
	0x1b7b,
	0x4300,
	{ 0xb7, 0x05, 0x28, 0xef, 0x0e, 0x2d, 0x7e, 0x72 }
};

// Listening defaults (VST identity = 0.0).
// 2026-08-03: nice_bulk_reinforce_v01 でノブが正味ゲインを持つようになり
// 効きが大幅に強くなった (bass=95% で 60Hz +10.4dB) ため、実機評価により
// 0.75 -> 0.30 へ引き下げ。
static constexpr float k_def_bass = 0.30f;
static constexpr float k_def_hi = 0.30f;

struct nice_params_t {
	uint8_t version = 1;
	uint8_t bypass = 0;
	float bass_amount = k_def_bass;
	float hi_amount = k_def_hi;
};

static void make_preset(const nice_params_t& p, dsp_preset& out) {
	dsp_preset_builder builder;
	builder << p.version << p.bypass << p.bass_amount << p.hi_amount;
	builder.finish(k_nice_guid, out);
}

static nice_params_t parse_preset(const dsp_preset& in) {
	nice_params_t p;
	try {
		dsp_preset_parser parser(in);
		parser >> p.version >> p.bypass >> p.bass_amount >> p.hi_amount;
		if (p.version != 1) {
			return nice_params_t{};
		}
	} catch (exception_io_data const&) {
		return nice_params_t{};
	}
	return p;
}

static void apply_params(FdgNiceBulk* inst, const nice_params_t& p) {
	if (!inst) {
		return;
	}
	fdg_nice_set_params(inst, p.bypass ? 1 : 0, p.bass_amount, p.hi_amount);
}

static void RunNiceConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback);

} // namespace

class dsp_fdg_nice : public dsp_impl_base {
public:
	dsp_fdg_nice(dsp_preset const& in) : m_params(parse_preset(in)) {}

	~dsp_fdg_nice() {
		if (m_inst) {
			fdg_nice_free(m_inst);
			m_inst = nullptr;
		}
	}

	static GUID g_get_guid() { return k_nice_guid; }

	static void g_get_name(pfc::string_base& p_out) { p_out = "fandango nice bulk"; }

	static bool g_get_default_preset(dsp_preset& p_out) {
		make_preset(nice_params_t{}, p_out);
		return true;
	}

	static bool g_have_config_popup() { return true; }

	static void g_show_config_popup(
		const dsp_preset& p_data,
		HWND p_parent,
		dsp_preset_edit_callback& p_callback) {
		RunNiceConfigPopup(p_data, p_parent, p_callback);
	}

	bool on_chunk(audio_chunk* chunk, abort_callback&) {
		if (chunk == nullptr) {
			return true;
		}
		const unsigned ch = chunk->get_channel_count();
		if (ch == 0) {
			return true;
		}

		const float srate = static_cast<float>(chunk->get_srate());
		ensure_instance(srate, ch);

		const t_size frames = chunk->get_sample_count();
		if (frames == 0 || m_inst == nullptr) {
			return true;
		}

		audio_sample* data = chunk->get_data();
		const t_size total = frames * ch;
		m_scratch.resize(total);
		for (t_size i = 0; i < total; ++i) {
			m_scratch[i] = static_cast<float>(data[i]);
		}

		fdg_nice_process(m_inst, m_scratch.data(), ch, frames);

		for (t_size i = 0; i < total; ++i) {
			data[i] = static_cast<audio_sample>(m_scratch[i]);
		}
		return true;
	}

	void on_endofplayback(abort_callback&) {}
	void on_endoftrack(abort_callback&) {}

	void flush() {
		if (m_inst) {
			fdg_nice_reset(m_inst, m_srate);
		}
	}

	double get_latency() { return 0; }
	bool need_track_change_mark() { return false; }

private:
	void ensure_instance(float srate, unsigned ch) {
		if (m_inst == nullptr) {
			m_inst = fdg_nice_new(srate, ch);
			m_srate = srate;
			m_ch = ch;
			apply_params(m_inst, m_params);
			return;
		}
		if (srate > 0.f && (srate != m_srate || ch != m_ch)) {
			fdg_nice_free(m_inst);
			m_inst = fdg_nice_new(srate, ch);
			m_srate = srate;
			m_ch = ch;
			apply_params(m_inst, m_params);
		}
	}

	FdgNiceBulk* m_inst = nullptr;
	float m_srate = 0.f;
	unsigned m_ch = 0;
	std::vector<float> m_scratch;
	nice_params_t m_params;
};

static dsp_factory_t<dsp_fdg_nice> g_fdg_nice_factory;

namespace {

class CNiceDSPPopup : public CDialogImpl<CNiceDSPPopup> {
public:
	CNiceDSPPopup(const dsp_preset& initData, dsp_preset_edit_callback& callback)
		: m_initData(initData), m_callback(callback) {}

	enum { IDD = IDD_DSP_NICE_BULK };

	// Map 0..100 UI ticks -> 0.0..1.0 param (matches VST max).
	// 上限は 2026-08-03 に 0.95 -> 1.0 へ拡張。0.95 は初期設計の発散懸念による
	// 安全マージンだったが、有理 clamp が有界性を保証するため根拠を失っていた
	// (alpha=1.0 でも単調性・有限性・固定点が厳密に成立することを実測で確認)。
	enum { RangeTotal = 100 };

	BEGIN_MSG_MAP_EX(CNiceDSPPopup)
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
		m_slider_bass = GetDlgItem(IDC_SLIDER_BASS);
		m_slider_hi = GetDlgItem(IDC_SLIDER_HI);
		m_slider_bass.SetRange(0, RangeTotal);
		m_slider_hi.SetRange(0, RangeTotal);
		apply_to_ui(parse_preset(m_initData));
		return TRUE;
	}

	void OnButton(UINT, int id, CWindow) { EndDialog(id); }

	void OnReset(UINT, int, CWindow) {
		apply_to_ui(nice_params_t{});
		push_live();
	}

	void OnCheck(UINT, int, CWindow) { push_live(); }
	void OnHScroll(UINT, UINT, CScrollBar) { push_live(); }

	static int float_to_pos(float v) {
		return pfc::clip_t<int>(pfc::rint32(v * 100.f), 0, RangeTotal);
	}

	static float pos_to_float(int pos) {
		return static_cast<float>(pos) / 100.f;
	}

	void apply_to_ui(const nice_params_t& p) {
		m_slider_bass.SetPos(float_to_pos(p.bass_amount));
		m_slider_hi.SetPos(float_to_pos(p.hi_amount));
		CheckDlgButton(IDC_CHECK_BYPASS, p.bypass ? BST_CHECKED : BST_UNCHECKED);
		RefreshLabels(p);
	}

	nice_params_t read_ui() const {
		nice_params_t p;
		p.bass_amount = pos_to_float(m_slider_bass.GetPos());
		p.hi_amount = pos_to_float(m_slider_hi.GetPos());
		p.bypass = IsDlgButtonChecked(IDC_CHECK_BYPASS) == BST_CHECKED ? 1 : 0;
		return p;
	}

	void push_live() {
		const nice_params_t p = read_ui();
		dsp_preset_impl preset;
		make_preset(p, preset);
		m_callback.on_preset_changed(preset);
		RefreshLabels(p);
	}

	void RefreshLabels(const nice_params_t& p) {
		pfc::string_formatter msg;
		msg << pfc::format_int(pfc::rint32(p.bass_amount * 100.f)) << "%";
		::uSetDlgItemText(*this, IDC_LABEL_BASS, msg);
		msg.reset();
		msg << pfc::format_int(pfc::rint32(p.hi_amount * 100.f)) << "%";
		::uSetDlgItemText(*this, IDC_LABEL_HI, msg);
	}

	const dsp_preset& m_initData;
	dsp_preset_edit_callback& m_callback;
	CTrackBarCtrl m_slider_bass;
	CTrackBarCtrl m_slider_hi;
	fb2k::CDarkModeHooks m_dark;
};

static void RunNiceConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback) {
	CNiceDSPPopup popup(p_data, p_callback);
	if (popup.DoModal(p_parent) != IDOK) {
		p_callback.on_preset_changed(p_data);
	}
}

} // namespace
