#include <stddef.h>
void *memmove(void *out,const void *in,size_t n)
{unsigned char *d=out;const unsigned char *s=in;if(d<s){for(size_t i=0;i<n;i++)d[i]=s[i];}else{while(n){n--;d[n]=s[n];}}return out;}
size_t strlen(const char *s){size_t n=0;while(s[n])n++;return n;}
int strcmp(const char *a,const char *b){while(*a && *a==*b){a++;b++;}return (unsigned char)*a-(unsigned char)*b;}
int strncmp(const char *a,const char *b,size_t n){while(n){if(*a!=*b || !*a)return (unsigned char)*a-(unsigned char)*b;a++;b++;n--;}return 0;}
char *strchr(const char *s,int c){do{if(*s==(char)c)return (char*)s;}while(*s++);return 0;}
int atoi(const char *s){int sign=1,n=0;while(*s==' ')s++;if(*s=='-'){sign=-1;s++;}while(*s>='0' && *s<='9' && n<100000000)n=n*10+*s++-'0';return n*sign;}
void *memcpy(void *out,const void *in,size_t n)
{unsigned char *d=out;const unsigned char *s=in;while(n--)*d++=*s++;return out;}
void *memset(void *out,int v,size_t n)
{unsigned char *d=out;while(n--)*d++=(unsigned char)v;return out;}
int memcmp(const void *a,const void *b,size_t n)
{const unsigned char *x=a,*y=b;while(n--){if(*x!=*y)return (int)*x-(int)*y;x++;y++;}return 0;}
char *strstr(const char *h,const char *n)
{if(!*n)return (char*)h;for(;*h;h++){const char *a=h,*b=n;while(*a && *a==*b){a++;b++;}if(!*b)return (char*)h;}return 0;}
