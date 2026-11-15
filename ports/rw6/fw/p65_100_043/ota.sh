set +e
setprop ota.progress.current 30
setprop ota.progress.next 30
setprop ota.version.next `getprop ota.version.current`

if [ ! -e /dev/audio ]
then
    echo "/dev/audio doesn't exist, will reboot to the old system" >> /dev/log
    setprop ota.progress.current -1
    exit
fi

if [ ! -e /dev/resource ]
then
    echo "/dev/resource doesn't exist, will reboot to the old system" >> /dev/log
    setprop ota.progress.current -1
    exit
fi

if [ ! -e /dev/sensor ]
then
    echo "/dev/sensor doesn't exist, will reboot to the old system" >> /dev/log
    setprop ota.progress.current -1
    exit
fi

if [ ! -e /dev/ap ]
then
    echo "/dev/ap doesn't exist, will reboot to the old system" >> /dev/log
    setprop ota.progress.current -1
    exit
fi

echo "install vela_audio.bin" >> /dev/log
time " dd if=/ota/vela_audio.bin of=/dev/audio bs=32768 verify"
if [ $? -ne 0 ]
then
    echo "dd vela_audio.bin failed" >> /dev/log
    reboot
fi
setprop ota.progress.current 30
setprop ota.progress.next 96

echo "install vela_resource.bin" >> /dev/log
time " dd if=/ota/vela_resource.bin of=/dev/resource bs=32768 verify"
if [ $? -ne 0 ]
then
    echo "dd vela_resource.bin failed" >> /dev/log
    reboot
fi
setprop ota.progress.current 96
setprop ota.progress.next 96

echo "install vela_sensor.bin" >> /dev/log
time " dd if=/ota/vela_sensor.bin of=/dev/sensor bs=32768 verify"
if [ $? -ne 0 ]
then
    echo "dd vela_sensor.bin failed" >> /dev/log
    reboot
fi
setprop ota.progress.current 96
setprop ota.progress.next 100

echo "install vela_ap.bin" >> /dev/log
time " dd if=/ota/vela_ap.bin of=/dev/ap bs=32768 verify"
if [ $? -ne 0 ]
then
    echo "dd vela_ap.bin failed" >> /dev/log
    reboot
fi
setprop ota.progress.current 100
