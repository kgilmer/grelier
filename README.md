# grelier

A desktop bar for Linux

## Overview

This program displays a fixed bar on the left-hand side of the screen.  It shows a list of all workspaces at the top and a user-configurable set of gauges at the bottom.  A gauge is something that displays system information or provides some action to change system state.  The program can take configuration parameters from CLI arguments or use Xresource-style key-value pairs specified in `$HOME/.config/grelier/Settings.xresources`.

## Status

This project is in active development and should not be considered stable.

## Usage

```
Usage: grelier [--gauges <gauges>] [--orientation <orientation>] [--theme <theme>] [--settings <settings>] [--list-settings]

Workspace + gauges display

Options:
  --gauges          clock, date, battery, cpu, disk, ram, net_up, net_down,
                    audio_out, audio_in, brightness, wifi
  --orientation     orientation of the bar (left or right)
  --theme           theme name:
                    CatppuccinFrappe,CatppuccinLatte,CatppuccinMacchiato,CatppuccinMocha,Dark,Dracula,Ferra,GruvboxDark,GruvboxLight,KanagawaDragon,KanagawaLotus,KanagawaWave,Light,Moonfly,Nightfly,Nord,Oxocarbon,TokyoNight,TokyoNightLight,TokyoNightStorm,AyuMirage
  --settings        comma-separated settings overrides (key=value,key2=value2)
  --list-settings   list settings for the selected gauges and exit
  --help, help      display usage information
```

## Gauges

### `audio_in`
- Purpose: Input volume control with mute toggle and device menu.
- Monitors/Measures: Default PulseAudio source volume and mute state.
- Settings: `grelier.audio_in.step_percent` (default `5`).

### `audio_out`
- Purpose: Output volume control with mute toggle and device menu.
- Monitors/Measures: Default PulseAudio sink volume and mute state.
- Settings: `grelier.audio_out.step_percent` (default `5`).

### `battery`
- Purpose: Battery status and charging indicator.
- Monitors/Measures: Battery capacity and charging state from udev power_supply.
- Settings: `grelier.battery.warning_percent` (default `49`), `grelier.battery.danger_percent` (default `19`).

### `brightness`
- Purpose: Backlight brightness indicator with scroll-based adjustment.
- Monitors/Measures: Backlight brightness percent via `/sys/class/backlight`.
- Settings: `grelier.brightness.step_percent` (default `5`), `grelier.brightness.refresh_interval_secs` (default `2`).

### `clock`
- Purpose: Wall-clock time readout.
- Monitors/Measures: Local system time (hour/minute, optional seconds).
- Settings: `grelier.clock.showseconds` (default `false`), `grelier.clock.hourformat` (default `24`).

### `cpu`
- Purpose: CPU utilization indicator with adaptive polling.
- Monitors/Measures: Aggregate CPU usage from `/proc/stat`.
- Settings: `grelier.cpu.quantitystyle` (default `grid`), `grelier.cpu.warning_threshold` (default `0.75`), `grelier.cpu.danger_threshold` (default `0.90`), `grelier.cpu.fast_threshold` (default `0.50`), `grelier.cpu.calm_ticks` (default `4`), `grelier.cpu.fast_interval_secs` (default `1`), `grelier.cpu.slow_interval_secs` (default `4`).

### `date`
- Purpose: Calendar date readout.
- Monitors/Measures: Local system date (month/day).
- Settings: `grelier.date.month_format` (default `%m`), `grelier.date.day_format` (default `%d`).

### `disk`
- Purpose: Disk usage indicator for a filesystem path.
- Monitors/Measures: Used/total space for the configured path.
- Settings: `grelier.disk.quantitystyle` (default `grid`), `grelier.disk.path` (default `/`), `grelier.disk.poll_interval_secs` (default `60`), `grelier.disk.warning_threshold` (default `0.85`), `grelier.disk.danger_threshold` (default `0.95`).

### `net_down`
- Purpose: Download throughput indicator.
- Monitors/Measures: Active interface receive rate from `/proc/net/dev`.
- Settings: Shared `grelier.net.*` settings (see `net_up`).

### `net_up`
- Purpose: Upload throughput indicator.
- Monitors/Measures: Active interface transmit rate from `/proc/net/dev`.
- Settings: `grelier.net.idle_threshold_bps` (default `10240`), `grelier.net.fast_interval_secs` (default `1`), `grelier.net.slow_interval_secs` (default `3`), `grelier.net.calm_ticks` (default `4`), `grelier.net.iface_cache_ttl_secs` (default `10`), `grelier.net.iface_ttl_secs` (default `5`), `grelier.net.sampler_min_interval_ms` (default `900`), `grelier.net.sys_class_net_path` (default `/sys/class/net`), `grelier.net.proc_net_route_path` (default `/proc/net/route`), `grelier.net.proc_net_dev_path` (default `/proc/net/dev`).

### `ram`
- Purpose: Memory utilization indicator with adaptive polling.
- Monitors/Measures: System RAM usage from `/proc/meminfo` (including shrinkable ZFS ARC).
- Settings: `grelier.ram.quantitystyle` (default `grid`), `grelier.ram.warning_threshold` (default `0.85`), `grelier.ram.danger_threshold` (default `0.95`), `grelier.ram.fast_threshold` (default `0.70`), `grelier.ram.calm_ticks` (default `4`), `grelier.ram.fast_interval_secs` (default `1`), `grelier.ram.slow_interval_secs` (default `4`).

### `wifi`
- Purpose: Wi-Fi link status and signal indicator.
- Monitors/Measures: Wi-Fi interface connection state and link quality from `/sys/class/net` and `/proc/net/wireless`.
- Settings: `grelier.wifi.quantitystyle` (default `grid`), `grelier.wifi.quality_max` (default `70`), `grelier.wifi.poll_interval_secs` (default `3`).

### `test_gauge`
- Purpose: Internal gauge for cycling quantity icons and attention states.
- Monitors/Measures: Synthetic values (no system monitoring).
- Settings: `grelier.test_gauge.quantitystyle` (default `pie`).
