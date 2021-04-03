#!/bin/sh

VOL=$(pamixer --get-volume-human)
ICON="volume_full"

if [ $VOL = "muted" ]
then
    ICON="volume_muted"
fi

echo [

echo {
if [ $VOL != "muted" ]
then
    echo '"text":"'$VOL' ",'
fi
echo '"icon":"'$ICON'",'
echo '"on_click": "pavucontrol",'
echo '"on_right_click": "pamixer -t",'
echo '"on_scroll_up": "pamixer -i 5",'
echo '"on_scroll_down": "pamixer -d 5"'
echo }

echo ]
