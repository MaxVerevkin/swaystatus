# swaystatus

Status command for i3bar/swaybar written in async rust. Based on i3status-rust. 

# Blocks

Currently `swaystatus` supports those blocks (TODO write docs):
- Time (`time`)
- Sway Keyboard Layout (`sway_kbd`)
- Temperature (`temperature`)
- Memory (`memory`)
- Cpu Utilization (`cpu`)
- GitHub (`github`)
- Network (`net`)
- WiFi (`wifi`)
- Cutom (`custom`)
- Music (`music`)
- Backlight (`backlight`)
- Focused Window (`forcused_window`)
- Weather (`weather`)
- Battery (`battery`)

Each block supports `on_click`, `on_click_sync`, `on_right_click` and `on_right_click_sync` options to handle bolcks' clicks. `_sync` options wait for command to finish before accepting any new clicks, while others don't.
