#ifndef CHAOS_BLUR_INTERACTION_H
#define CHAOS_BLUR_INTERACTION_H
#include <stdint.h>
/* Keep interaction work scalar. No saved object pointers, GPU calls or clocks
 * inside draw callbacks beyond the existing monotonic clock accessor. A quiet
 * window also handles inertia and missing END without a permanently stuck gate. */
#define CHAOS_BLUR_QUIET_MS 160u
struct chaos_blur_interaction { uint32_t last,events; unsigned valid; };
static inline void chaos_blur_interaction_note(struct chaos_blur_interaction *s,uint32_t now,unsigned code) {
    if(code!=9u && code!=11u && code!=12u) return;
    s->last=now;s->valid=1;++s->events;
}
static inline int chaos_blur_interaction_busy(const struct chaos_blur_interaction *s,uint32_t now) {
    return s->valid && (uint32_t)(now-s->last)<CHAOS_BLUR_QUIET_MS;
}
#endif
