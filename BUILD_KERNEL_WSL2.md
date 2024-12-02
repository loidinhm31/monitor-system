# Guide: Building Custom WSL2 Kernel

This guide walks through the process of building a custom kernel for WSL2 (Windows Subsystem for Linux 2). This can be
useful for enabling additional kernel features, applying custom patches, or updating to a specific kernel version.

## Prerequisites

- WSL2 installed and running
- sudo privileges
- Basic development tools (git, make, gcc, etc.)
- At least 15GB of free disk space
- Windows directory mounted and accessible from WSL2

## Step 1: Determine Current Kernel Version

First, check your current WSL2 kernel version:

```bash
uname -r
```

This will output something like `5.15.167.4`. We'll use this version number throughout the build process.

## Step 2: Set Up Build Environment

Set the kernel version variable automatically from your current kernel version:

```bash
VERSION=$(uname -r)
# The command above will set VERSION to your current kernel version (e.g., 5.15.167.4)
```

## Step 3: Clone Microsoft's WSL2 Kernel Repository

Clone the specific version branch of Microsoft's WSL2 kernel repository:

```bash
sudo git clone -b linux-msft-wsl-${VERSION} https://github.com/microsoft/WSL2-Linux-Kernel.git ${VERSION}-microsoft-standard
cd ${VERSION}-microsoft-standard
```

This command:

- Clones the specific version branch (`linux-msft-wsl-5.15.167.4`)
- Creates a directory named with the version number
- Changes into the newly created directory

## Step 4: Copy and Prepare Current Kernel Configuration

Copy the current kernel configuration and prepare it for modification:

```bash
sudo cp /proc/config.gz config.gz
sudo gunzip config.gz
sudo mv config .config
```

These commands:

1. Copy the compressed kernel config from `/proc/config.gz`
2. Decompress the configuration file
3. Rename it to `.config` (the standard kernel configuration filename)

## Step 5: Configure Kernel Options

Run the kernel configuration interface:

```bash
sudo make menuconfig
```

This opens a text-based configuration interface. Use these keys to navigate:

- Arrow keys: Move through options
- Enter: Enter a submenu or select
- Space: Toggle option states (* = built-in, M = module, empty = disabled)
- ESC-ESC: Go back/exit current menu
- '/': Search for options
- 'Q': Save and quit

Make the following configurations:

1. **General setup**
    - Navigate to "Local version"
    - Add the suffix "-usb-add" (or your preferred suffix)
    - This helps identify your custom kernel later

2. **Device Drivers → Multimedia support**
    - Set to built-in (*) using Space
    - Enter the submenu and configure:
        - Set "Filter media drivers" to built-in (*)
        - Set "Autoselect ancillary drivers" to built-in (*)
        - Set "Media device types - Cameras and video grabbers" to built-in (*)
        - Set "Media drivers - Media USB Adapters" to built-in (*) and enter its submenu:
            - Set "GSPCA based webcams" to module (M)
            - Set "USB Video Class (UVC)" to module (M)
            - Enter "GSPCA based webcams" and set ALL USB camera drivers to module (M)

3. **Device Drivers → USB support**
    - Set to built-in (*) using Space
    - Enter the submenu and configure:
        - Set "Support for Host-side USB" to built-in (*)
        - Set "USB/IP support" to built-in (*) and enter its submenu:
            - Set ALL subitems to built-in (*)

After making these changes:

`Select "Yes" `to save the new configuration

## Step 6: Build the Kernel

Build the kernel and its modules:

```bash
sudo make -j$(nproc)
sudo make modules_install -j$(nproc)
sudo make install -j$(nproc)
```

These commands:

1. Build the kernel using all available CPU cores
2. Install the kernel modules
3. Install the kernel itself

The `-j$(nproc)` flag enables parallel compilation using all available CPU cores.

## Step 7: Copy Kernel to Windows

Create a directory in Windows to store the kernel and copy it there:

```bash
sudo mkdir /mnt/c/Sources
sudo cp vmlinux /mnt/c/Sources/
```

## Step 8: Configure WSL to Use the New Kernel

1. Create or edit `%UserProfile%\.wslconfig` in Windows:

```ini
[wsl2]
kernel=C:\\Sources\\vmlinux
```

2. Restart WSL2 from PowerShell with admin privileges:

```powershell
wsl --shutdown
```

## Verification

After restarting WSL2, verify the new kernel is in use:

```bash
uname -r
```

The output should match the version you built.

## Troubleshooting

1. If build fails with missing dependencies:
   ```bash
   sudo apt install build-essential flex bison libssl-dev libelf-dev
   ```

2. If kernel fails to load:
    - Check kernel path in `.wslconfig`
    - Ensure file permissions are correct
    - Verify the build completed successfully

3. If you need to revert:
    - Remove or comment out the kernel line in `.wslconfig`
    - Restart WSL2