//! VST3 ホストを介さず、実プラグインと同じ GUI を確認する開発用エントリポイント。

use fandango_trip_delay::TripDelayPlugin;
use nih_plug::prelude::*;

fn main() {
    nih_export_standalone::<TripDelayPlugin>();
}
