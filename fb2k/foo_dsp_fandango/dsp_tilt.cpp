#include "stdafx.h"
#include "fdg_ffi.h"
#include "resource.h"

#include <helpers/DarkMode.h>

#include <vector>

namespace {

// 4c768955-c973-4782-9941-42d319a0a3b4
static constexpr GUID k_tilt_guid = {
	0x4c768955,
	0xc973,
	0x4782,
	{ 0x99, 0x41, 0x42, 0xd3, 0x19, 0xa0, 0xa3, 0xb4 }
};

// Listening defaults (VST gain=0 identity).
static constexpr float k_def_gain = 1.5f;
static constexpr float k_def_pivot = 700.0f;
static constexpr float k_def_slope = 0.7f;

struct tilt_params_t {
	uint8_t version = 1;
	uint8_t bypass = 0;
	float gain = k_def_gain;
	float pivot = k_def_pivot;
	float slope = k_def_slope;
};

static void make_preset(const tilt_params_t& p, dsp_preset& out) {
	dsp_preset_builder builder;
	builder << p.version << p.bypass << p.gain << p.pivot << p.slope;
	builder.finish(k_tilt_guid, out);
}

static tilt_params_t parse_preset(const dsp_preset& in) {
	tilt_params_t p;
	try {
		dsp_preset_parser parser(in);
		parser >> p.version >> p.bypass >> p.gain >> p.pivot >> p.slope;
		if (p.version != 1) {
			return tilt_params_t{};
		}
	} catch (exception_io_data const&) {
		return tilt_params_t{};
	}
	return p;
}

static void apply_params(FdgTilt* inst, const tilt_params_t& p) {
	if (!inst) {
		return;
	}
	fdg_tilt_set_params(inst, p.bypass ? 1 : 0, p.gain, p.pivot, p.slope);
}

static void RunTiltConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback);

} // namespace

class dsp_fdg_tilt : public dsp_impl_base {
public:
	dsp_fdg_tilt(dsp_preset const& in) : m_params(parse_preset(in)) {}

	~dsp_fdg_tilt() {
		if (m_inst) {
			fdg_tilt_free(m_inst);
			m_inst = nullptr;
		}
	}

	static GUID g_get_guid() { return k_tilt_guid; }
	static void g_get_name(pfc::string_base& p_out) { p_out = "fandango tilt"; }
	static bool g_get_default_preset(dsp_preset& p_out) {
		make_preset(tilt_params_t{}, p_out);
		return true;
	}
	static bool g_have_config_popup() { return true; }
	static void g_show_config_popup(
		const dsp_preset& p_data,
		HWND p_parent,
		dsp_preset_edit_callback& p_callback) {
		RunTiltConfigPopup(p_data, p_parent, p_callback);
	}

	bool on_chunk(audio_chunk* chunk, abort_callback&) {
		if (!chunk) {
			return true;
		}
		const unsigned ch = chunk->get_channel_count();
		if (ch == 0) {
			return true;
		}
		const float srate = static_cast<float>(chunk->get_srate());
		ensure_instance(srate, ch);
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
		fdg_tilt_process(m_inst, m_scratch.data(), ch, frames);
		for (t_size i = 0; i < total; ++i) {
			data[i] = static_cast<audio_sample>(m_scratch[i]);
		}
		return true;
	}

	void on_endofplayback(abort_callback&) {}
	void on_endoftrack(abort_callback&) {}
	void flush() {
		if (m_inst) {
			fdg_tilt_reset(m_inst, m_srate);
		}
	}
	double get_latency() { return 0; }
	bool need_track_change_mark() { return false; }

private:
	void ensure_instance(float srate, unsigned ch) {
		if (!m_inst) {
			m_inst = fdg_tilt_new(srate, ch);
			m_srate = srate;
			m_ch = ch;
			apply_params(m_inst, m_params);
			return;
		}
		if (srate > 0.f && (srate != m_srate || ch != m_ch)) {
			fdg_tilt_free(m_inst);
			m_inst = fdg_tilt_new(srate, ch);
			m_srate = srate;
			m_ch = ch;
			apply_params(m_inst, m_params);
		}
	}

	FdgTilt* m_inst = nullptr;
	float m_srate = 0.f;
	unsigned m_ch = 0;
	std::vector<float> m_scratch;
	tilt_params_t m_params;
};

static dsp_factory_t<dsp_fdg_tilt> g_fdg_tilt_factory;

namespace {

class CTiltDSPPopup : public CDialogImpl<CTiltDSPPopup> {
public:
	CTiltDSPPopup(const dsp_preset& initData, dsp_preset_edit_callback& callback)
		: m_initData(initData), m_callback(callback) {}

	enum { IDD = IDD_DSP_TILT };
	// gain: 0..240 => -12.0..+12.0 dB (0.1 step); pivot 100..5000; slope 30..100 => 0.30..1.00
	enum {
		GainRange = 240,
		GainMinDb10 = -120,
		PivotMin = 100,
		PivotMax = 5000,
		SlopeMin = 30,
		SlopeMax = 100
	};

	BEGIN_MSG_MAP_EX(CTiltDSPPopup)
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
		m_gain = GetDlgItem(IDC_SLIDER_GAIN);
		m_pivot = GetDlgItem(IDC_SLIDER_PIVOT);
		m_slope = GetDlgItem(IDC_SLIDER_SLOPE);
		m_gain.SetRange(0, GainRange);
		m_pivot.SetRange(PivotMin, PivotMax);
		m_slope.SetRange(SlopeMin, SlopeMax);
		apply_to_ui(parse_preset(m_initData));
		return TRUE;
	}

	void OnButton(UINT, int id, CWindow) { EndDialog(id); }
	void OnReset(UINT, int, CWindow) {
		apply_to_ui(tilt_params_t{});
		push_live();
	}
	void OnCheck(UINT, int, CWindow) { push_live(); }
	void OnHScroll(UINT, UINT, CScrollBar) { push_live(); }

	static int gain_to_pos(float db) {
		const int t = pfc::rint32(db * 10.f) - GainMinDb10;
		return pfc::clip_t<int>(t, 0, GainRange);
	}
	static float pos_to_gain(int pos) {
		return static_cast<float>(pos + GainMinDb10) / 10.f;
	}

	void apply_to_ui(const tilt_params_t& p) {
		m_gain.SetPos(gain_to_pos(p.gain));
		m_pivot.SetPos(pfc::clip_t<int>(pfc::rint32(p.pivot), PivotMin, PivotMax));
		m_slope.SetPos(pfc::clip_t<int>(pfc::rint32(p.slope * 100.f), SlopeMin, SlopeMax));
		CheckDlgButton(IDC_CHECK_BYPASS, p.bypass ? BST_CHECKED : BST_UNCHECKED);
		RefreshLabels(p);
	}

	tilt_params_t read_ui() const {
		tilt_params_t p;
		p.gain = pos_to_gain(m_gain.GetPos());
		p.pivot = static_cast<float>(m_pivot.GetPos());
		p.slope = static_cast<float>(m_slope.GetPos()) / 100.f;
		p.bypass = IsDlgButtonChecked(IDC_CHECK_BYPASS) == BST_CHECKED ? 1 : 0;
		return p;
	}

	void push_live() {
		const tilt_params_t p = read_ui();
		dsp_preset_impl preset;
		make_preset(p, preset);
		m_callback.on_preset_changed(preset);
		RefreshLabels(p);
	}

	void RefreshLabels(const tilt_params_t& p) {
		pfc::string_formatter msg;
		msg << pfc::format_float(p.gain, 0, 1) << " dB";
		::uSetDlgItemText(*this, IDC_LABEL_GAIN, msg);
		msg.reset();
		msg << pfc::format_int(pfc::rint32(p.pivot)) << " Hz";
		::uSetDlgItemText(*this, IDC_LABEL_PIVOT, msg);
		msg.reset();
		msg << pfc::format_float(p.slope, 0, 2);
		::uSetDlgItemText(*this, IDC_LABEL_SLOPE, msg);
	}

	const dsp_preset& m_initData;
	dsp_preset_edit_callback& m_callback;
	CTrackBarCtrl m_gain, m_pivot, m_slope;
	fb2k::CDarkModeHooks m_dark;
};

static void RunTiltConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback) {
	CTiltDSPPopup popup(p_data, p_callback);
	if (popup.DoModal(p_parent) != IDOK) {
		p_callback.on_preset_changed(p_data);
	}
}

} // namespace
