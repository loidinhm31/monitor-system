sudo v4l2-ctl --list-devices

sudo chmod 777 /dev/video0

cargo install cross

sudo apt-get install libclang-dev
sudo apt-get install libopencv-dev
sudo apt-get install clang
sudo apt-get install libstdc++-12-dev

sudo apt-get install -y libasound2-dev

// Raspberry Pi 4 
apt-get install gcc-arm-linux-gnueabihf
apt-get install g++-arm-linux-gnueabihf


cargo install cross
cross run --target x86_64-pc-windows-gnu






// Config audio for Raspberry Pi
```sh
arecord -l
```

```sh
pcm.usbmic {
    type hw
    card <your-card-number>
}

ctl.usbmic {
    type hw
    card <your-card-number>
}

pcm.!default {
    type asym
    playback.pcm {
        type plug
        slave.pcm "hw:0,0"
    }
    capture.pcm {
        type plug
        slave.pcm "usbmic"
    }
}
```

# Docker
```shell
docker exec -it <container_name> /bin/sh
```

make up_build PLATFORM=amd64