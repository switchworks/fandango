#include "stdafx.h"
#include "fdg_ffi.h"
#include "resource.h"

#include <helpers/DarkMode.h>

#include <vector>

namespace {

// Generated for this component -- do not copy sample GUIDs.
// 7cbd619c-3dd9-4c75-b07b-969b41c3dd37
static constexpr GUID k_spread_guid = {
	0x7cbd619c,
	0x3dd9,
	0x4c75,
	{ 0xb0, 0x7b, 0x96, 0x9b, 0x41, 0xc3, 0xdd, 0x37 }
};

struct spread_params_t {
	uint8_t version = 1;
	uint8_t bypass = 0;
	float expander = 0.66f;
	uint8_t inverse = 0;
	float center_amount = 1.0f;
};

static void make_preset(const spread_params_t& p, dsp_preset& out) {
	dsp_preset_builder builder;
	builder << p.version << p.bypass << p.expander << p.inverse << p.center_amount;
	builder.finish(k_spread_guid, out);
}

static spread_params_t parse_preset(const dsp_preset& in) {
	spread_params_t p;
	try {
		dsp_preset_parser parser(in);
		parser >> p.version >> p.bypass >> p.expander >> p.inverse >> p.center_amount;
		if (p.version != 1) {
			return spread_params_t{};
		}
	} catch (exception_io_data const&) {
		return spread_params_t{};
	}
	return p;
}

static void apply_params(FdgSpread* inst, const spread_params_t& p) {
	if (!inst) {
		return;
	}
	fdg_spread_set_params(
		inst,
		p.bypass ? 1 : 0,
		p.expander,
		p.inverse ? 1 : 0,
		p.center_amount);
}

static void RunSpreadConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback);

} // namespace

class dsp_fdg_spread : public dsp_impl_base {
public:
	dsp_fdg_spread(dsp_preset const& in) : m_params(parse_preset(in)) {}

	~dsp_fdg_spread() {
		if (m_inst) {
			fdg_spread_free(m_inst);
			m_inst = nullptr;
		}
	}

	static GUID g_get_guid() { return k_spread_guid; }

	static void g_get_name(pfc::string_base& p_out) { p_out = "fandango spread"; }

	static bool g_get_default_preset(dsp_preset& p_out) {
		make_preset(spread_params_t{}, p_out);
		return true;
	}

	static bool g_have_config_popup() { return true; }

	static void g_show_config_popup(
		const dsp_preset& p_data,
		HWND p_parent,
		dsp_preset_edit_callback& p_callback) {
		RunSpreadConfigPopup(p_data, p_parent, p_callback);
	}

	bool on_chunk(audio_chunk* chunk, abort_callback&) {
		if (chunk == nullptr) {
			return true;
		}
		const unsigned ch = chunk->get_channel_count();
		if (ch != 2) {
			return true;
		}

		const float srate = static_cast<float>(chunk->get_srate());
		ensure_instance(srate);

		const t_size frames = chunk->get_sample_count();
		if (frames == 0 || m_inst == nullptr) {
			return true;
		}

		audio_sample* data = chunk->get_data();
		m_l.resize(frames);
		m_r.resize(frames);

		for (t_size i = 0; i < frames; ++i) {
			m_l[i] = static_cast<float>(data[i * 2 + 0]);
			m_r[i] = static_cast<float>(data[i * 2 + 1]);
		}

		fdg_spread_process(m_inst, m_l.data(), m_r.data(), frames);

		for (t_size i = 0; i < frames; ++i) {
			data[i * 2 + 0] = static_cast<audio_sample>(m_l[i]);
			data[i * 2 + 1] = static_cast<audio_sample>(m_r[i]);
		}
		return true;
	}

	void on_endofplayback(abort_callback&) {}
	void on_endoftrack(abort_callback&) {}

	void flush() {
		if (m_inst) {
			fdg_spread_reset(m_inst, m_srate);
		}
	}

	double get_latency() { return 0; }
	bool need_track_change_mark() { return false; }

private:
	void ensure_instance(float srate) {
		if (m_inst == nullptr) {
			m_inst = fdg_spread_new(srate);
			m_srate = srate;
			apply_params(m_inst, m_params);
			return;
		}
		if (srate > 0.f && srate != m_srate) {
			fdg_spread_reset(m_inst, srate);
			m_srate = srate;
			apply_params(m_inst, m_params);
		}
	}

	FdgSpread* m_inst = nullptr;
	float m_srate = 0.f;
	std::vector<float> m_l;
	std::vector<float> m_r;
	spread_params_t m_params;
};

static dsp_factory_t<dsp_fdg_spread> g_fdg_spread_factory;

namespace {

class CSpreadDSPPopup : public CDialogImpl<CSpreadDSPPopup> {
public:
	CSpreadDSPPopup(const dsp_preset& initData, dsp_preset_edit_callback& callback)
		: m_initData(initData), m_callback(callback) {}

	enum { IDD = IDD_DSP_SPREAD };

	enum {
		RangeTotal = 100
	};

	BEGIN_MSG_MAP_EX(CSpreadDSPPopup)
		MSG_WM_INITDIALOG(OnInitDialog)
		COMMAND_HANDLER_EX(IDOK, BN_CLICKED, OnButton)
		COMMAND_HANDLER_EX(IDCANCEL, BN_CLICKED, OnButton)
		COMMAND_HANDLER_EX(IDC_RESET, BN_CLICKED, OnReset)
		COMMAND_HANDLER_EX(IDC_CHECK_BYPASS, BN_CLICKED, OnCheck)
		COMMAND_HANDLER_EX(IDC_CHECK_INVERSE, BN_CLICKED, OnCheck)
		MSG_WM_HSCROLL(OnHScroll)
	END_MSG_MAP()

private:
	BOOL OnInitDialog(CWindow, LPARAM) {
		m_dark.AddDialogWithControls(m_hWnd);

		m_slider_exp = GetDlgItem(IDC_SLIDER_EXPANDER);
		m_slider_ctr = GetDlgItem(IDC_SLIDER_CENTER);
		m_slider_exp.SetRange(0, RangeTotal);
		m_slider_ctr.SetRange(0, RangeTotal);

		apply_to_ui(parse_preset(m_initData));
		return TRUE;
	}

	void OnButton(UINT, int id, CWindow) {
		EndDialog(id);
	}

	void OnReset(UINT, int, CWindow) {
		apply_to_ui(spread_params_t{});
		push_live();
	}

	void OnCheck(UINT, int, CWindow) {
		push_live();
	}

	void OnHScroll(UINT, UINT, CScrollBar) {
		push_live();
	}

	void apply_to_ui(const spread_params_t& p) {
		m_slider_exp.SetPos(float_to_pos(p.expander));
		m_slider_ctr.SetPos(float_to_pos(p.center_amount));
		CheckDlgButton(IDC_CHECK_BYPASS, p.bypass ? BST_CHECKED : BST_UNCHECKED);
		CheckDlgButton(IDC_CHECK_INVERSE, p.inverse ? BST_CHECKED : BST_UNCHECKED);
		RefreshLabels(p);
	}

	static int float_to_pos(float v) {
		return pfc::clip_t<int>(pfc::rint32(v * RangeTotal), 0, RangeTotal);
	}

	static float pos_to_float(int pos) {
		return static_cast<float>(pos) / static_cast<float>(RangeTotal);
	}

	spread_params_t read_ui() const {
		spread_params_t p;
		p.expander = pos_to_float(m_slider_exp.GetPos());
		p.center_amount = pos_to_float(m_slider_ctr.GetPos());
		p.bypass = IsDlgButtonChecked(IDC_CHECK_BYPASS) == BST_CHECKED ? 1 : 0;
		p.inverse = IsDlgButtonChecked(IDC_CHECK_INVERSE) == BST_CHECKED ? 1 : 0;
		return p;
	}

	void push_live() {
		const spread_params_t p = read_ui();
		dsp_preset_impl preset;
		make_preset(p, preset);
		m_callback.on_preset_changed(preset);
		RefreshLabels(p);
	}

	void RefreshLabels(const spread_params_t& p) {
		pfc::string_formatter msg;
		msg << pfc::format_int(pfc::rint32(p.expander * 100.f)) << "%";
		::uSetDlgItemText(*this, IDC_LABEL_EXPANDER, msg);
		msg.reset();
		msg << pfc::format_int(pfc::rint32(p.center_amount * 100.f)) << "%";
		::uSetDlgItemText(*this, IDC_LABEL_CENTER, msg);
	}

	const dsp_preset& m_initData;
	dsp_preset_edit_callback& m_callback;
	CTrackBarCtrl m_slider_exp;
	CTrackBarCtrl m_slider_ctr;
	fb2k::CDarkModeHooks m_dark;
};

static void RunSpreadConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback) {
	CSpreadDSPPopup popup(p_data, p_callback);
	if (popup.DoModal(p_parent) != IDOK) {
		p_callback.on_preset_changed(p_data);
	}
}

} // namespace
