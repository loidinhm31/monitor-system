sudo v4l2-ctl --list-devices

sudo chmod 777 /dev/video0


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


# Get the WSL2 IP address:
```shell
ip addr show eth0 | grep "inet\b" | awk '{print $2}' | cut -d/ -f1
```

Set up port forwarding from Windows to WSL2.
```shell
netsh interface portproxy add v4tov4 listenport=8081 listenaddress=0.0.0.0 connectport=8081 connectaddress=<WSL2_IP>
```

Verify the port forwarding:
```shell
netsh interface portproxy show all
```

If cannot access, just need to set `.wslconfig`
```shell
[wsl2]
networkingMode=mirrored
```