#include <assert.h>
#include "../shared/keypad_display.h"
static void packet(uint8_t p[20],unsigned seq,unsigned row,char ch){memset(p,ch,20);p[0]=1;p[1]=seq;p[2]=row;p[19]=0;}
int main(void){
 KdFrame f={0};uint8_t p[20];
 packet(p,1,1,'A');assert(!kd_receive(&f,p,20,0));
 for(unsigned row=0;row<4;row++){packet(p,1,row,'A'+row);assert(kd_receive(&f,p,20,row));assert(f.valid==(row==3));}
 assert(kd_fresh(&f,8002));assert(!kd_fresh(&f,8003));
 assert(f.lines[3][15]=='D'&&f.lines[3][16]==0);
 packet(p,2,0,'X');assert(kd_receive(&f,p,20,9000));assert(f.lines[0][0]=='A');
 packet(p,1,1,'Y');assert(!kd_receive(&f,p,20,9001));
 packet(p,2,1,'Y');assert(!kd_receive(&f,p,20,11001));
 packet(p,3,0,'Q');p[3]=0;assert(!kd_receive(&f,p,20,12000));
 packet(p,3,0,'Q');assert(!kd_receive(&f,p,19,12000));
 p[0]=2;assert(!kd_receive(&f,p,20,12000));
 for(unsigned seq=254;seq<258;seq++)for(unsigned row=0;row<4;row++){
  packet(p,seq,row,'Z');assert(kd_receive(&f,p,20,13000));
 }
 assert(f.sequence==1&&f.lines[0][0]=='Z');
}
