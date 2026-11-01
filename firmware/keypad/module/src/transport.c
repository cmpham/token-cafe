/* SPDX-License-Identifier: MIT */
#include <stdio.h>
#include <zephyr/kernel.h>
#include <zephyr/bluetooth/gatt.h>
#include <zephyr/device.h>
#include <zephyr/sys/atomic.h>
#include <zmk/ble.h>
#include <zmk/usb.h>
#include <raw_hid/events.h>
#include <zmk/events/keycode_state_changed.h>
#include "keypad_display.h"
#include "transport.h"

/* Mac-only service; the retired board OLED UUIDs are no longer exposed. */
#define MAC_UUID(n) BT_UUID_INIT_128(0x01,0xe6,0x8d,0x1d,0x38,0x58,0x7d,0x9b,0xd9,0x4c,0x67,0x7f,n,0xfe,0xca,0xf6)
static struct bt_uuid_128 service_uuid=MAC_UUID(0x10), identity_uuid=MAC_UUID(0x11),
    display_uuid=MAC_UUID(0x12), input_uuid=MAC_UUID(0x13);
static KdFrame usb_frame, ble_frame;
K_MUTEX_DEFINE(frame_lock);
static atomic_t usb_seen, usb_active, ble_seen, ble_active;
static uint8_t input_sequence;
static bool recent(atomic_t *active, atomic_t *seen) {
    return atomic_get(active) && (uint32_t)(k_uptime_get_32()-(uint32_t)atomic_get(seen))<5000;
}
static bool usb_preferred(void) {
    return zmk_usb_is_hid_ready() && recent(&usb_active,&usb_seen);
}
static void host_seen(atomic_t *active, atomic_t *seen) {
    atomic_set(seen,(atomic_val_t)k_uptime_get_32()); atomic_set(active,1);
}
/* USB vendor reports preserve VIA; no input is sent to the board or OS keyboard. */
bool token_cafe_raw_hid(const struct raw_hid_received_event *event) {
    const uint8_t *p=event->data;
    if(event->length!=32 || p[0]!=0xfe || memcmp(p+1,"TCO",3)) return false;
    uint8_t reply[32]={0}; memcpy(reply,p,6);
    bool ok=false;
    if(p[4]==1 && event->transport==RAW_HID_TRANSPORT_USB && zmk_usb_is_hid_ready()) {
        if(p[5]==0) ok=true;
        else if(p[5]==1) {
            k_mutex_lock(&frame_lock,K_FOREVER);
            ok=kd_receive(&usb_frame,p+6,KD_PACKET_SIZE,k_uptime_get());
            k_mutex_unlock(&frame_lock);
        } else if(p[5]==3) {
            /* Explicit physical-USB pairing reset, never requested automatically. */
            zmk_ble_clear_bonds(); ok=true;
        }
    }
    reply[6]=ok?1:0; reply[7]=1;
    if(ok) host_seen(&usb_active,&usb_seen);
    raise_raw_hid_sent_event((struct raw_hid_sent_event){
        .data=reply,.length=sizeof(reply),.transport=event->transport});
    return true;
}
static bool selected_peer(struct bt_conn *conn) {
    struct bt_conn *active=zmk_ble_active_profile_conn();
    bool matches=active && active==conn && bt_conn_get_security(conn)>=BT_SECURITY_L2;
    if(active) bt_conn_unref(active);
    return matches;
}
static ssize_t identity(struct bt_conn *conn,const struct bt_gatt_attr *attr,
                        void *buf,uint16_t len,uint16_t offset) {
    if(!selected_peer(conn)) return BT_GATT_ERR(BT_ATT_ERR_AUTHORIZATION);
    static const uint8_t value[]="TCM2651A6626B5AE5931";
    return bt_gatt_attr_read(conn,attr,buf,len,offset,value,sizeof(value)-1);
}
static ssize_t receive(struct bt_conn *conn,const struct bt_gatt_attr *attr,
                       const void *buf,uint16_t len,uint16_t offset,uint8_t flags) {
    ARG_UNUSED(attr); ARG_UNUSED(flags);
    if(offset) return BT_GATT_ERR(BT_ATT_ERR_INVALID_OFFSET);
    if(!selected_peer(conn)) return BT_GATT_ERR(BT_ATT_ERR_AUTHORIZATION);
    k_mutex_lock(&frame_lock,K_FOREVER);
    bool ok=kd_receive(&ble_frame,buf,len,k_uptime_get());
    k_mutex_unlock(&frame_lock);
    if(ok) host_seen(&ble_active,&ble_seen);
    return ok ? len : BT_GATT_ERR(BT_ATT_ERR_VALUE_NOT_ALLOWED);
}
BT_GATT_SERVICE_DEFINE(cafe_mac,
    BT_GATT_PRIMARY_SERVICE(&service_uuid),
    BT_GATT_CHARACTERISTIC(&identity_uuid.uuid,BT_GATT_CHRC_READ,
        BT_GATT_PERM_READ_ENCRYPT,identity,NULL,NULL),
    BT_GATT_CHARACTERISTIC(&display_uuid.uuid,BT_GATT_CHRC_WRITE,
        BT_GATT_PERM_WRITE_ENCRYPT,NULL,receive,NULL),
    BT_GATT_CHARACTERISTIC(&input_uuid.uuid,BT_GATT_CHRC_NOTIFY,
        BT_GATT_PERM_NONE,NULL,NULL,NULL),
    BT_GATT_CCC(NULL,BT_GATT_PERM_READ_ENCRYPT|BT_GATT_PERM_WRITE_ENCRYPT));
static void disconnected(struct bt_conn *conn,uint8_t reason) {
    ARG_UNUSED(conn); ARG_UNUSED(reason);
    atomic_set(&ble_active,0);
    k_mutex_lock(&frame_lock,K_FOREVER); memset(&ble_frame,0,sizeof(ble_frame)); k_mutex_unlock(&frame_lock);
}
BT_CONN_CB_DEFINE(cafe_connection)={.disconnected=disconnected};
/* Consume every dedicated control on both transports, even while disconnected.
 * Never queue inputs for later, and choose exactly one host transport per press. */
bool token_cafe_usb_keycode(const struct zmk_keycode_state_changed *ev) {
    uint8_t command=0;
    if(ev->usage_page==7) {
        switch(ev->keycode) {
        case 0x29: command=1; break; case 0x2c: command=2; break;
        case 0x2a: command=3; break; case 0x52: command=4; break;
        case 0x28: command=5; break; case 0x50: command=6; break;
        case 0x51: command=7; break; case 0x4f: command=8; break;
        case 0x68: command=9; break;
        }
    } else if(ev->usage_page==12) {
        if(ev->keycode==0xea) command=10;
        if(ev->keycode==0xe9) command=11;
    }
    if(!command) return false;
    if(!ev->state) return true;
    uint8_t report[32]={0xfe,'T','C','O',1,2,command,++input_sequence};
    if(usb_preferred()) {
        raise_raw_hid_sent_event((struct raw_hid_sent_event){
            .data=report,.length=sizeof(report),.transport=RAW_HID_TRANSPORT_USB});
    } else if(recent(&ble_active,&ble_seen)) {
        struct bt_conn *conn=zmk_ble_active_profile_conn();
        if(conn) {
            if(selected_peer(conn) && bt_gatt_is_subscribed(conn,&cafe_mac.attrs[6],BT_GATT_CCC_NOTIFY))
                bt_gatt_notify(conn,&cafe_mac.attrs[6],report,8);
            bt_conn_unref(conn);
        }
    }
    return true;
}
/* Copy a complete frame while holding the lock; LVGL never runs in a BLE callback. */
void token_cafe_display_lines(char lines[4][17]) {
    k_mutex_lock(&frame_lock,K_FOREVER);
    KdFrame *frame=usb_preferred()?&usb_frame:&ble_frame;
    if(kd_fresh(frame,k_uptime_get())) memcpy(lines,frame->lines,sizeof(frame->lines));
    else if(frame->valid) {
        strcpy(lines[0],"CAFE OFFLINE"); strcpy(lines[2],"RECONNECT HOST");
    }
    k_mutex_unlock(&frame_lock);
}
