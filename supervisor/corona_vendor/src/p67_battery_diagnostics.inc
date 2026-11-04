/* Exact .043 battery diagnostics. Observe latched integers only; never query
 * objects, the GPU, or resources here. The owner calls log() from its UI poll,
 * not from DRAW_MAIN/DELETE, and diagnostics never change rendering state. */
#ifndef P67_BATTERY_DIAGNOSTICS_INC
#define P67_BATTERY_DIAGNOSTICS_INC

#define P67_BATTERY_DIAG_BYTES 2048u
#define P67_BATTERY_DIAG_ATTEMPTS 8u
#define P67_BATTERY_DIAG_COOLDOWN 32u
#define P67_BATTERY_DIAG_PATH "/data/offlinelog/resource-hook-p67-battery.log"

enum p67_battery_diag_reason {
    P67_BAT_NONE = 0, P67_BAT_DISABLED, P67_BAT_IDLE, P67_BAT_BINDING,
    P67_BAT_KEY, P67_BAT_STYLE, P67_BAT_INFO, P67_BAT_SOURCE_SHAPE,
    P67_BAT_ALLOC, P67_BAT_CREATE, P67_BAT_SNAPSHOT, P67_BAT_FENCE,
    P67_BAT_BUFFER, P67_BAT_IMAGE_SHAPE, P67_BAT_NONBLACK,
    P67_BAT_EMPTY_ALPHA, P67_BAT_STALE, P67_BAT_CHARGE_SHAPE,
    P67_BAT_MAIN_ALLOC, P67_BAT_MAIN_SNAPSHOT, P67_BAT_UNMATTE
};
struct p67_battery_sample_diag {
    uint32_t reason, source, need_w, need_h, info_rc, info_w, info_h, info_cf;
    uint32_t image_w, image_h, draw_w, draw_h, extra, snapshot_rc;
    uint32_t buffer_flags, buffer_w, buffer_h, alpha_min, alpha_max;
    uint32_t alpha_zero, alpha_nonzero, bad_x, bad_y, bad_rgba;
};
static struct p67_battery_sample_diag p67_battery_sample_diag[2];
/* Modes: disabled, preparing, ready, error, waiting. Kinds: mask, charge,
 * main capture, ordinary display draw. Unknown values remain printable. */
static uint32_t p67_battery_diag_mode, p67_battery_diag_reason, p67_battery_diag_kind;
static uint32_t p67_battery_diag_style[4], p67_battery_diag_main[4];
static uint32_t p67_battery_diag_draws, p67_battery_diag_blocked;

static void p67_battery_diag_fail(uint32_t reason, uint32_t kind) {
    p67_battery_diag_reason = reason;
    p67_battery_diag_kind = kind;
    p67_battery_diag_mode = reason == P67_BAT_DISABLED ? 0u :
        reason == P67_BAT_IDLE || reason == P67_BAT_BINDING ? 4u : 3u;
}

#define P67_BATTERY_SAMPLE_FIELDS(X) \
    X(reason) X(source) X(need_w) X(need_h) X(info_rc) X(info_w) X(info_h) X(info_cf) \
    X(image_w) X(image_h) X(draw_w) X(draw_h) X(extra) X(snapshot_rc) \
    X(buffer_flags) X(buffer_w) X(buffer_h) X(alpha_min) X(alpha_max) \
    X(alpha_zero) X(alpha_nonzero) X(bad_x) X(bad_y) X(bad_rgba)
#define P67_BATTERY_DIAG_SAMPLE_WORDS 24u
/* Explicit field access avoids assuming that struct fields form a C array. */
static void p67_battery_diag_sample_values(const struct p67_battery_sample_diag *s,
                                          uint32_t out[P67_BATTERY_DIAG_SAMPLE_WORDS]) {
    unsigned i = 0;
#define X(field) out[i++] = s->field;
    P67_BATTERY_SAMPLE_FIELDS(X)
#undef X
}

struct p67_battery_diag_writer {
    char *out;
    uint32_t used, capacity;
    unsigned failed;
};
static void p67_battery_diag_text(struct p67_battery_diag_writer *w, const char *text) {
    while (*text) {
        if (w->used >= w->capacity - 1u) { w->failed = 1; return; }
        if (w->out) w->out[w->used] = *text;
        ++w->used; ++text;
    }
}
static void p67_battery_diag_number(struct p67_battery_diag_writer *w, uint32_t value) {
    char digits[10];
    unsigned count = 0;
    do { digits[count++] = (char)('0' + value % 10u); value /= 10u; } while (value);
    while (count) {
        char digit[2] = {digits[--count], 0};
        p67_battery_diag_text(w, digit);
    }
}
static void p67_battery_diag_field(struct p67_battery_diag_writer *w,
                                   const char *prefix, const char *name, uint32_t value) {
    p67_battery_diag_text(w, prefix); p67_battery_diag_text(w, name);
    p67_battery_diag_text(w, "="); p67_battery_diag_number(w, value);
    p67_battery_diag_text(w, "\n");
}
/* All-or-nothing, NUL-terminated output; returned size excludes NUL. A small
 * capacity returns zero without touching the caller's buffer. No native calls,
 * I/O, state changes, printf, or 64-bit arithmetic/runtime imports. */
static uint32_t p67_battery_diag_snapshot(char *out, uint32_t capacity) {
    static const char *const modes[] = {"disabled", "preparing", "ready", "error", "waiting"};
    static const char *const kinds[] = {"mask", "charge", "main", "draw"};
    static const char *const reasons[] = {
        "none", "disabled", "idle", "binding", "key", "style", "info", "source-shape",
        "alloc", "create", "snapshot", "fence", "buffer", "image-shape", "nonblack",
        "empty-alpha", "stale", "charge-shape", "main-alloc", "main-snapshot", "unmatte"
    };
    static const char *const sample_keys[] = {
#define X(field) #field,
        P67_BATTERY_SAMPLE_FIELDS(X)
#undef X
    };
    static const char *const styles[] = {"style_68", "style_70", "style_104", "style_105"};
    static const char *const main_keys[] = {"main_w", "main_h", "main_extra", "main_flags"};
    struct p67_battery_diag_writer w = {0, 0, 0, 0};
    uint32_t values[P67_BATTERY_DIAG_SAMPLE_WORDS];
    unsigned pass, i, sample;
    if (!out || !capacity) return 0;
    w.capacity = capacity < P67_BATTERY_DIAG_BYTES ? capacity : P67_BATTERY_DIAG_BYTES;
    for (pass = 0; pass < 2u; ++pass) {
        w.used = 0;
        p67_battery_diag_text(&w, "P67BATTERY2\nbuild=alpha-strict-diag-1\nmode=");
        p67_battery_diag_text(&w, p67_battery_diag_mode < 5u ? modes[p67_battery_diag_mode] : "unknown");
        p67_battery_diag_text(&w, "\nreason=");
        p67_battery_diag_text(&w, p67_battery_diag_reason < sizeof(reasons) / sizeof(reasons[0]) ?
                              reasons[p67_battery_diag_reason] : "unknown");
        p67_battery_diag_text(&w, "\nkind=");
        p67_battery_diag_text(&w, p67_battery_diag_kind < 4u ? kinds[p67_battery_diag_kind] : "unknown");
        p67_battery_diag_text(&w, "\n");
#define FIELD(name, value) p67_battery_diag_field(&w, "", (name), (uint32_t)(value))
        FIELD("mode_code", p67_battery_diag_mode);
        FIELD("reason_code", p67_battery_diag_reason);
        FIELD("kind_code", p67_battery_diag_kind);
        FIELD("object", p67_battery_obj); FIELD("root", p67_battery_root);
        FIELD("status", p67_battery_status); FIELD("buffer", p67_battery_buffer);
        FIELD("ready", p67_battery_ready);
        FIELD("mask_cached_source", p67_battery_mask_source); FIELD("mask_ready", p67_battery_mask_ready);
        FIELD("charge_cached_source", p67_battery_charge_source); FIELD("charge_ready", p67_battery_charge_ready);
        FIELD("generation", p67_battery_generation);
        FIELD("draws", p67_battery_diag_draws); FIELD("blocked_draws", p67_battery_diag_blocked);
        for (i = 0; i < 4u; ++i) {
            FIELD(styles[i], p67_battery_diag_style[i]); FIELD(main_keys[i], p67_battery_diag_main[i]);
        }
#undef FIELD
        for (sample = 0; sample < 2u; ++sample) {
            p67_battery_diag_sample_values(&p67_battery_sample_diag[sample], values);
            for (i = 0; i < P67_BATTERY_DIAG_SAMPLE_WORDS; ++i)
                p67_battery_diag_field(&w, sample ? "charge_" : "mask_", sample_keys[i], values[i]);
        }
        p67_battery_diag_text(&w, "snapshot_complete=1\n");
        if (w.failed) return 0;
        if (!pass) w.out = out;
    }
    out[w.used] = 0;
    return w.used;
}

#ifndef P67_BATTERY_DIAG_WRITE
/* Provider seam returns 1 only for a complete write, 0 on any failure. It is
 * invoked solely by log(), at the serialized UI-poll boundary. fd=0 is valid. */
static int p67_battery_diag_native_write(const char *data, uint32_t size) {
    uint32_t used = 0;
    int fd = rh_platform_open(P67_BATTERY_DIAG_PATH, RH_FILE_REPLACE), written;
    if (fd < 0) return 0;
    while (used < size) {
        written = rh_platform_write(fd, data + used, size - used);
        if (written <= 0 || (uint32_t)written > size - used) break;
        used += (uint32_t)written;
    }
    rh_platform_close(fd);
    return used == size;
}
#define P67_BATTERY_DIAG_WRITE(data, size) p67_battery_diag_native_write((data), (size))
#endif

/* Signature excludes changing draw counters/generation. Readiness, reasons,
 * source identity, actual styles/geometry, and measurements remain observable.
 * Fixed word counts keep storage and traversal bounded without a bytewise
 * compare of compiler-dependent struct padding. */
#define P67_BATTERY_DIAG_SIGNATURE_WORDS (20u + 2u * P67_BATTERY_DIAG_SAMPLE_WORDS)
static unsigned p67_battery_diag_attempts, p67_battery_diag_cooldown;
static unsigned p67_battery_diag_logged, p67_battery_diag_recovery_used;
static uint32_t p67_battery_diag_previous_mode;
static uint32_t p67_battery_diag_logged_signature[P67_BATTERY_DIAG_SIGNATURE_WORDS];
static char p67_battery_diag_output[P67_BATTERY_DIAG_BYTES];
static void p67_battery_diag_signature(uint32_t out[P67_BATTERY_DIAG_SIGNATURE_WORDS]) {
    unsigned i, sample, used = 0;
    const uint32_t core[] = {p67_battery_diag_mode, p67_battery_diag_reason, p67_battery_diag_kind,
        p67_battery_obj, p67_battery_root, p67_battery_status, p67_battery_buffer,
        p67_battery_ready, p67_battery_mask_source, p67_battery_mask_ready,
        p67_battery_charge_source, p67_battery_charge_ready};
    for (i = 0; i < sizeof(core) / sizeof(core[0]); ++i) out[used++] = core[i];
    for (i = 0; i < 4u; ++i) out[used++] = p67_battery_diag_style[i];
    for (i = 0; i < 4u; ++i) out[used++] = p67_battery_diag_main[i];
    for (sample = 0; sample < 2u; ++sample) {
        p67_battery_diag_sample_values(&p67_battery_sample_diag[sample], out + used);
        used += P67_BATTERY_DIAG_SAMPLE_WORDS;
    }
}
static void p67_battery_diag_log(void) {
    uint32_t signature[P67_BATTERY_DIAG_SIGNATURE_WORDS], size;
    unsigned i, equal, recovery, complete;
    recovery = p67_battery_diag_mode == 2u && p67_battery_diag_previous_mode == 3u &&
               !p67_battery_diag_recovery_used;
    p67_battery_diag_previous_mode = p67_battery_diag_mode;
    if (p67_battery_diag_cooldown) --p67_battery_diag_cooldown;
    p67_battery_diag_signature(signature);
    equal = p67_battery_diag_logged;
    for (i = 0; equal && i < P67_BATTERY_DIAG_SIGNATURE_WORDS; ++i)
        equal = signature[i] == p67_battery_diag_logged_signature[i];
    if (equal || (p67_battery_diag_cooldown && !recovery)) return;
    size = p67_battery_diag_snapshot(p67_battery_diag_output, sizeof(p67_battery_diag_output));
    if (!size) return;
    if (recovery) p67_battery_diag_recovery_used = 1;
    if (p67_battery_diag_attempts < P67_BATTERY_DIAG_ATTEMPTS) ++p67_battery_diag_attempts;
    /* O_TRUNC can leave partial output. Do not retain an old dedup signature
     * after any attempted replacement; failed writes repair after 32 polls. */
    p67_battery_diag_logged = 0;
    complete = P67_BATTERY_DIAG_WRITE(p67_battery_diag_output, size) == 1;
    if (!complete || p67_battery_diag_attempts >= P67_BATTERY_DIAG_ATTEMPTS)
        p67_battery_diag_cooldown = P67_BATTERY_DIAG_COOLDOWN;
    if (!complete) return;
    for (i = 0; i < P67_BATTERY_DIAG_SIGNATURE_WORDS; ++i)
        p67_battery_diag_logged_signature[i] = signature[i];
    p67_battery_diag_logged = 1;
}
#undef P67_BATTERY_SAMPLE_FIELDS
#endif
