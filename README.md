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


Each block supports multiple `click` options to handle bolcks' clicks.

Example:

```toml
[[block]]
block = "time"
[[block.click]]
button = "left" # Which button to handle
cmd = "kitty" # The shell command to run
sync = false # Whether to wait for command to finish before proceeding (default is false)
update = true # Whether to update the block after click (default is true)
```
