#pragma once
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
typedef struct Engine Engine;
Engine *cafe_engine_new(const uint8_t *device,size_t len,const uint8_t *key,size_t capacity,bool injection);
void cafe_engine_drop(Engine *);
void cafe_engine_disconnect(Engine *);
size_t cafe_engine_auth_read(Engine *,uint8_t *,size_t);
bool cafe_engine_auth_write(Engine *,const uint8_t *,size_t,uint64_t);
bool cafe_engine_receive(Engine *,const uint8_t *,size_t,uint64_t);
size_t cafe_engine_next(Engine *,uint8_t *,size_t);
bool cafe_engine_tick(Engine *,uint64_t);
bool cafe_engine_touch(Engine *,uint8_t,int32_t,int32_t,uint64_t);
bool cafe_engine_approval_button(Engine *,bool,uint64_t);
bool cafe_engine_approval_waiting(Engine *);
bool cafe_engine_render(Engine *,uint16_t *,uint64_t);
void cafe_engine_presented(Engine *,uint64_t);
uint32_t cafe_engine_status(Engine *);
bool cafe_engine_journal(Engine *,const uint8_t *,size_t);

bool cafe_engine_draw(Engine *, uint64_t);
void cafe_engine_pack(Engine *, uint16_t *);

bool cafe_engine_feedback(Engine *, uint16_t *);

void cafe_engine_io_diagnostic(Engine *,uint16_t *,bool);
