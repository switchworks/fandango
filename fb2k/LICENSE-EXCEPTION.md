# fandango — foobar2000 component: additional permission

**Additional permission under GNU GPL version 3, section 7.**

Copyright (C) 2026 switchworks

fandango as a whole is distributed under the GNU General Public License,
version 3 or later (see `LICENSE` at the repository root).

The **foobar2000 component** — the sources under `fb2k/` and
`crates/ffi/`, together with the fandango sources those pull in —
is distributed under that same license, with the following additional
permission granted under section 7 of the GPL:

> You have permission to link or combine this work with the Windows
> Template Library (WTL), which is licensed under the Microsoft Public
> License (Ms-PL), and to convey the resulting combined work. You may
> extend this permission to a modified version of this work, but you are
> not obliged to do so. If you do not wish to do so, delete this
> permission statement from your version.

## Why this exists

The foobar2000 component's configuration dialogs are built on WTL
(`CDialogImpl`). WTL is under the Ms-PL, which the Free Software
Foundation considers incompatible with the GPL. Without this additional
permission, the combined binary could not satisfy both licenses at once
and therefore could not be distributed.

Granting an additional permission is the copyright holder's own act. It
adds a permission; it removes nothing. The copyleft of the fandango
sources is unchanged: a derivative of this component is still governed by
the GPL.

This permission is limited to the foobar2000 component. **The VST3 build
is not covered and does not need it** — it links vst3-sys (GPL-3) and no
Ms-PL code.

## What this does not do

This statement does not alter the license of any third-party code. WTL,
the foobar2000 SDK, `pfc`, and `libPPUI` each carry their own terms; see
`licenses/NON-CARGO.md` for what they are and where they came from.
