# USB headset descriptor probe

Separate ESP32-S3 diagnostic image: uses the native USB-OTG interface as a host
and logs on UART0 through the board's COM connector. It reads USB descriptors;
it neither starts audio streaming nor records microphone data.

From the repository root:

```sh
bash tools/firmware.sh usb-probe-build
bash tools/firmware.sh usb-probe-flash PORT
target/esp/tools/python_env/idf5.4_py3.12_env/bin/python firmware/usb-probe/read_log.py PORT
```

Keep COM connected to the computer; attach the headset to USB-OTG. The log prints
VID/PID, standard descriptors and raw class-specific descriptor bytes. These
identify audio class version, formats and endpoint directions. Enumeration alone
does not prove successful playback or microphone operation. No enumeration does
not prove headset incompatibility: connector wiring, USB-C role negotiation and
VBUS power must also be checked on third-party boards.

This image replaces the screenless café firmware. Restore it with:

```sh
bash tools/firmware.sh hardware-flash PORT
bash tools/firmware.sh hardware-test PORT
```
