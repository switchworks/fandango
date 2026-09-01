#include "stdafx.h"
#include "fdg_ffi.h"
#include "resource.h"

#include <helpers/DarkMode.h>

#include <vector>

namespace {

// 9e3af1c7-2b84-4d6e-a05f-7c1d8e42b930
static constexpr GUID k_finalist_guid = {
	0x9e3af1c7,
	0x2b84,
	0x4d6e,
	{ 0xa0, 0x5f, 0x7c, 0x1d, 0x8e, 0x42, 0xb9, 0x30 }
};

// input only differs from VST3 (+6 dB for listening convenience).
// drive / ceiling / release match the VST3 defaults.
static constexpr float k_def_input_db = 6.0f;
static constexpr float k_def_drive = 0.0f;
static constexpr float k_def_ceiling_db = -1.0f;
static constexpr float k_def_release_ms = 80.0f;

struct finalist_params_t {
	uint8_t version = 1;
	uint8_t bypass = 0;
	float input_db = k_def_input_db;
	float drive = k_def_drive;
	float ceiling_db = k_def_ceiling_db;
	float release_ms = k_def_release_ms;
};

static void make_preset(const finalist_params_t& p, dsp_preset& out) {
	dsp_preset_builder builder;
	builder << p.version << p.bypass << p.input_db << p.drive << p.ceiling_db
		<< p.release_ms;
	builder.finish(k_finalist_guid, out);
}

static finalist_params_t parse_preset(const dsp_preset& in) {
	finalist_params_t p;
	try {
		dsp_preset_parser parser(in);
		parser >> p.version >> p.bypass >> p.input_db >> p.drive >> p.ceiling_db
			>> p.release_ms;
		if (p.version != 1) {
			return finalist_params_t{};
		}
	} catch (exception_io_data const&) {
		return finalist_params_t{};
	}
	return p;
}

static void apply_params(FdgFinalist* inst, const finalist_params_t& p) {
	if (!inst) {
		return;
	}
	fdg_finalist_set_params(
		inst, p.bypass ? 1 : 0, p.input_db, p.drive, p.ceiling_db, p.release_ms);
}

static void RunFinalistConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback);

} // namespace

class dsp_fdg_finalist : public dsp_impl_base {
public:
	dsp_fdg_finalist(dsp_preset const& in) : m_params(parse_preset(in)) {}

	~dsp_fdg_finalist() {
		if (m_inst) {
			fdg_finalist_free(m_inst);
			m_inst = nullptr;
		}
	}

	static GUID g_get_guid() { return k_finalist_guid; }
	static void g_get_name(pfc::string_base& p_out) { p_out = "fandango finalist"; }
	static bool g_get_default_preset(dsp_preset& p_out) {
		make_preset(finalist_params_t{}, p_out);
		return true;
	}
	static bool g_have_config_popup() { return true; }
	static void g_show_config_popup(
		const dsp_preset& p_data,
		HWND p_parent,
		dsp_preset_edit_callback& p_callback) {
		RunFinalistConfigPopup(p_data, p_parent, p_callback);
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
		m_chan_config = chunk->get_channel_config();
		m_srate_i = chunk->get_srate();
		audio_sample* data = chunk->get_data();
		const t_size total = frames * ch;
		m_scratch.resize(total);
		for (t_size i = 0; i < total; ++i) {
			m_scratch[i] = static_cast<float>(data[i]);
		}
		fdg_finalist_process(m_inst, m_scratch.data(), ch, frames);
		for (t_size i = 0; i < total; ++i) {
			data[i] = static_cast<audio_sample>(m_scratch[i]);
		}
		m_primed = true;
		return true;
	}

	// Output lags by D = 2N-2 samples (lookahead). Push silence at the end to
	// drain it; otherwise every track loses its last D samples.
	void on_endofplayback(abort_callback&) { flush_tail(); }
	void on_endoftrack(abort_callback&) { flush_tail(); }

	void flush() {
		if (m_inst) {
			fdg_finalist_reset(m_inst, m_srate);
		}
		m_primed = false;
	}

	double get_latency() {
		if (!m_inst || m_srate <= 0.f) {
			return 0;
		}
		return static_cast<double>(fdg_finalist_latency(m_inst))
			/ static_cast<double>(m_srate);
	}

	// Must be true so that on_endoftrack() is called (needed for tail flush).
	bool need_track_change_mark() { return true; }

private:
	void ensure_instance(float srate, unsigned ch) {
		if (!m_inst) {
			m_inst = fdg_finalist_new(srate, ch);
			m_srate = srate;
			m_ch = ch;
			apply_params(m_inst, m_params);
			return;
		}
		if (srate > 0.f && (srate != m_srate || ch != m_ch)) {
			fdg_finalist_free(m_inst);
			m_inst = fdg_finalist_new(srate, ch);
			m_srate = srate;
			m_ch = ch;
			apply_params(m_inst, m_params);
		}
	}

	void flush_tail() {
		if (!m_inst || !m_primed || m_ch == 0 || m_srate_i == 0) {
			return;
		}
		const t_size frames = fdg_finalist_latency(m_inst);
		if (frames == 0) {
			return;
		}
		const t_size total = frames * m_ch;
		m_scratch.assign(total, 0.f);
		fdg_finalist_process(m_inst, m_scratch.data(), m_ch, frames);
		m_tail.resize(total);
		for (t_size i = 0; i < total; ++i) {
			m_tail[i] = static_cast<audio_sample>(m_scratch[i]);
		}
		audio_chunk* out = insert_chunk(total);
		out->set_data(m_tail.data(), frames, m_ch, m_srate_i, m_chan_config);
		// Delay line is drained; drop state so the next track starts with an empty window.
		fdg_finalist_reset(m_inst, m_srate);
		m_primed = false;
	}

	FdgFinalist* m_inst = nullptr;
	float m_srate = 0.f;
	unsigned m_srate_i = 0;
	unsigned m_ch = 0;
	unsigned m_chan_config = 0;
	bool m_primed = false;
	std::vector<float> m_scratch;
	std::vector<audio_sample> m_tail;
	finalist_params_t m_params;
};

static dsp_factory_t<dsp_fdg_finalist> g_fdg_finalist_factory;

namespace {

class CFinalistDSPPopup : public CDialogImpl<CFinalistDSPPopup> {
public:
	CFinalistDSPPopup(const dsp_preset& initData, dsp_preset_edit_callback& callback)
		: m_initData(initData), m_callback(callback) {}

	enum { IDD = IDD_DSP_FINALIST };
	// input: 0..240 => -12.0..+12.0 dB (0.1 step)
	// drive: 0..100 => 0.00..1.00
	// ceiling: 0..120 => -12.0..0.0 dB (0.1 step)
	// release: 10..500 ms
	enum {
		InputRange = 240,
		InputMinDb10 = -120,
		DriveMin = 0,
		DriveMax = 100,
		CeilingRange = 120,
		CeilingMinDb10 = -120,
		ReleaseMin = 10,
		ReleaseMax = 500
	};

	BEGIN_MSG_MAP_EX(CFinalistDSPPopup)
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
		m_input = GetDlgItem(IDC_SLIDER_INPUT);
		m_drive = GetDlgItem(IDC_SLIDER_DRIVE);
		m_ceiling = GetDlgItem(IDC_SLIDER_CEILING);
		m_release = GetDlgItem(IDC_SLIDER_RELEASE);
		m_input.SetRange(0, InputRange);
		m_drive.SetRange(DriveMin, DriveMax);
		m_ceiling.SetRange(0, CeilingRange);
		m_release.SetRange(ReleaseMin, ReleaseMax);
		apply_to_ui(parse_preset(m_initData));
		return TRUE;
	}

	void OnButton(UINT, int id, CWindow) { EndDialog(id); }
	void OnReset(UINT, int, CWindow) {
		apply_to_ui(finalist_params_t{});
		push_live();
	}
	void OnCheck(UINT, int, CWindow) { push_live(); }
	void OnHScroll(UINT, UINT, CScrollBar) { push_live(); }

	static int db_to_pos(float db, int min_db10, int range) {
		const int t = pfc::rint32(db * 10.f) - min_db10;
		return pfc::clip_t<int>(t, 0, range);
	}
	static float pos_to_db(int pos, int min_db10) {
		return static_cast<float>(pos + min_db10) / 10.f;
	}

	void apply_to_ui(const finalist_params_t& p) {
		m_input.SetPos(db_to_pos(p.input_db, InputMinDb10, InputRange));
		m_drive.SetPos(
			pfc::clip_t<int>(pfc::rint32(p.drive * 100.f), DriveMin, DriveMax));
		m_ceiling.SetPos(db_to_pos(p.ceiling_db, CeilingMinDb10, CeilingRange));
		m_release.SetPos(
			pfc::clip_t<int>(pfc::rint32(p.release_ms), ReleaseMin, ReleaseMax));
		CheckDlgButton(IDC_CHECK_BYPASS, p.bypass ? BST_CHECKED : BST_UNCHECKED);
		RefreshLabels(p);
	}

	finalist_params_t read_ui() const {
		finalist_params_t p;
		p.input_db = pos_to_db(m_input.GetPos(), InputMinDb10);
		p.drive = static_cast<float>(m_drive.GetPos()) / 100.f;
		p.ceiling_db = pos_to_db(m_ceiling.GetPos(), CeilingMinDb10);
		p.release_ms = static_cast<float>(m_release.GetPos());
		p.bypass = IsDlgButtonChecked(IDC_CHECK_BYPASS) == BST_CHECKED ? 1 : 0;
		return p;
	}

	void push_live() {
		const finalist_params_t p = read_ui();
		dsp_preset_impl preset;
		make_preset(p, preset);
		m_callback.on_preset_changed(preset);
		RefreshLabels(p);
	}

	void RefreshLabels(const finalist_params_t& p) {
		pfc::string_formatter msg;
		msg << pfc::format_float(p.input_db, 0, 1) << " dB";
		::uSetDlgItemText(*this, IDC_LABEL_INPUT, msg);
		msg.reset();
		msg << pfc::format_float(p.drive, 0, 2);
		::uSetDlgItemText(*this, IDC_LABEL_DRIVE, msg);
		msg.reset();
		msg << pfc::format_float(p.ceiling_db, 0, 1) << " dB";
		::uSetDlgItemText(*this, IDC_LABEL_CEILING, msg);
		msg.reset();
		msg << pfc::format_int(pfc::rint32(p.release_ms)) << " ms";
		::uSetDlgItemText(*this, IDC_LABEL_RELEASE, msg);
	}

	const dsp_preset& m_initData;
	dsp_preset_edit_callback& m_callback;
	CTrackBarCtrl m_input, m_drive, m_ceiling, m_release;
	fb2k::CDarkModeHooks m_dark;
};

static void RunFinalistConfigPopup(
	const dsp_preset& p_data,
	HWND p_parent,
	dsp_preset_edit_callback& p_callback) {
	CFinalistDSPPopup popup(p_data, p_callback);
	if (popup.DoModal(p_parent) != IDOK) {
		p_callback.on_preset_changed(p_data);
	}
}

} // namespace
