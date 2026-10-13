#!/usr/bin/env bash
# Build/run the simulator or screenless hardware firmware using the project-local ESP-IDF install.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
export IDF_TOOLS_PATH="${IDF_TOOLS_PATH:-$ROOT/target/esp/tools}"
export IDF_PATH="${IDF_PATH:-$ROOT/target/esp/esp-idf}"
export IDF_PYTHON_ENV_PATH="${IDF_PYTHON_ENV_PATH:-$IDF_TOOLS_PATH/python_env/idf5.4_py3.12_env}"
export PATH="$IDF_PYTHON_ENV_PATH/bin:$PATH"
if [[ ! -f "$IDF_PATH/export.sh" ]]; then
    echo 'ESP-IDF is missing. See firmware/README.md for installation.' >&2
    exit 1
fi
# ESP-IDF export expects a conventional interactive-shell environment.
set +u
source "$IDF_PATH/export.sh" >/dev/null
set -u
cd "$ROOT/firmware"
case "${1:-build}" in
    build)
        idf.py build
        cd build
        python -m esptool --chip esp32s3 merge_bin --fill-flash-size 16MB -o merged-binary.bin @flash_args
        ;;
    qa-build|qa-flash)
        python "$ROOT/tools/provision_ble.py"
        cd ble
        flash_args=(build)
        if [[ "$1" == qa-flash ]]; then
            [[ $# == 2 ]] || exit 2
            flash_args=(-p "$2" -b 460800 flash)
        fi
        idf.py -B build-qa -D SDKCONFIG=sdkconfig.qa \
            -D 'SDKCONFIG_DEFAULTS=sdkconfig.defaults;sdkconfig.jc4827.defaults;sdkconfig.qa.defaults' "${flash_args[@]}"
        ;;
    display-build|display-flash)
        python "$ROOT/tools/provision_ble.py"
        cd ble
        flash_args=()
        if [[ "$1" == display-flash ]]; then
            [[ $# == 2 ]] || { echo 'display-flash requires an explicit serial port' >&2; exit 2; }
            flash_args=(-p "$2" -b 460800 flash)
        else
            flash_args=(build)
        fi
        idf.py -B build-jc4827 -D SDKCONFIG=sdkconfig.jc4827 \
            -D 'SDKCONFIG_DEFAULTS=sdkconfig.defaults;sdkconfig.jc4827.defaults' "${flash_args[@]}"
        ;;
    ble-build)
        python "$ROOT/tools/provision_ble.py"
        cd ble
        idf.py build
        ;;
    ble-flash)
        python "$ROOT/tools/provision_ble.py"
        [[ $# == 2 ]] || { echo 'ble-flash requires an explicit serial port' >&2; exit 2; }
        cd ble
        idf.py -p "$2" -b 460800 flash
        ;;
    usb-probe-build)
        cd usb-probe
        idf.py build
        ;;
    usb-probe-flash)
        [[ $# == 2 ]] || { echo 'usb-probe-flash requires an explicit serial port' >&2; exit 2; }
        cd usb-probe
        idf.py -p "$2" -b 460800 flash
        ;;
    hardware-build)
        idf.py -B build-hardware -D SDKCONFIG=sdkconfig.hardware \
            -D 'SDKCONFIG_DEFAULTS=sdkconfig.defaults;sdkconfig.hardware.defaults' build
        ;;
    hardware-info)
        [[ $# == 2 ]] || { echo 'hardware-info requires an explicit serial port' >&2; exit 2; }
        python -m esptool --chip esp32s3 --port "$2" flash_id
        ;;
    hardware-flash)
        [[ $# == 2 ]] || { echo 'hardware-flash requires an explicit serial port' >&2; exit 2; }
        grep -qx 'CONFIG_TC_HEADLESS=y' sdkconfig.hardware
        idf.py -B build-hardware -D SDKCONFIG=sdkconfig.hardware \
            -D 'SDKCONFIG_DEFAULTS=sdkconfig.defaults;sdkconfig.hardware.defaults' -p "$2" -b 460800 flash
        ;;
    hardware-test) exec python "$ROOT/firmware/hardware.py" "${@:2}" ;;
    run) exec python "$ROOT/firmware/host.py" run "${@:2}" ;;
    test) exec python "$ROOT/firmware/host.py" test "${@:2}" ;;
    *) echo 'usage: bash tools/firmware.sh {display-build|display-flash PORT|ble-build|ble-flash PORT|build|run|test|hardware-build|hardware-info PORT|hardware-flash PORT|hardware-test PORT} [host options]' >&2; exit 2 ;;
esac
