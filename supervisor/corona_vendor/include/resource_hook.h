#ifndef RESOURCE_HOOK_H
#define RESOURCE_HOOK_H
#include <stdint.h>
#define RH_RULES 256
#define RH_PATH 256
/* Keep the legacy root available for one-time import. Native builds resolve
 * the fixed corona module's private root through get_data_path before UI I/O. */
#ifndef RH_NATIVE_MANAGER
#define RH_NATIVE_MANAGER 0
#endif
#if defined(RH_TARGET_1043) && RH_TARGET_1043
#define RH_LEGACY_APP_FILES_ROOT "/data/files/ng.lst.corona/"
#else
#define RH_LEGACY_APP_FILES_ROOT "/data/quickapp/files/ng.lst.corona/"
#endif
#define RH_NATIVE_APP_FILES_ROOT "/data/canopus/m.corona/"
#if RH_NATIVE_MANAGER
#define RH_APP_FILES_ROOT RH_NATIVE_APP_FILES_ROOT
#else
#define RH_APP_FILES_ROOT RH_LEGACY_APP_FILES_ROOT
#endif
#define RH_CONFIG_PATH RH_APP_FILES_ROOT "mappings.tsv"
/* Shared status-v1 / reload-v1 / optional reload-v2 channel. Reload v2 adds
 * memory status to the receipt; legacy RHRS1 and descriptor ABI stay unchanged.
 * Wire framing and ownership are documented in docs/MODULE_CONTROL.md. */
#define RH_CONTROL_REQUEST_PATH RH_APP_FILES_ROOT "control.request"
#define RH_CONTROL_RESPONSE_PATH RH_APP_FILES_ROOT "control.response"
#define RH_THEME_ROOT RH_APP_FILES_ROOT "themes/"
#define RH_SYSTEM_DESTINATION "@system"
#define RH_QUICKAPP_ICON_PREFIX "@quickapp-icon/"
/* Directory rules map source prefixes into the app-private themes tree and
 * append the unmatched suffix; file rules map one exact source path. The
 * reserved RH_SYSTEM_DESTINATION exact-file rule explicitly keeps the firmware
 * resource and masks any broader directory rule. The longest match wins. */
struct rh_rule { char source[RH_PATH], destination[RH_PATH]; };
#define RH_INDEX_DIRECTORY 1u
#define RH_INDEX_SYSTEM 2u
#define RH_INDEX_QUICKAPP 4u
/* NULL unless source is a valid package-qualified icon declaration. */
const char *rh_quickapp_package(const char *source);
struct rh_indexed_rule {
    uint16_t source_offset, destination_offset;
    uint8_t source_length, destination_length, flags, reserved;
};
/* Immutable after construction except references, which the owner changes only
 * under its lock. The string pool immediately follows count indexed records. */
struct rh_snapshot {
    uint32_t references, allocation_bytes, pool_bytes, count;
    struct rh_indexed_rule rules[];
};
struct rh_mapping_view {
    const struct rh_rule *rules;
    uint32_t count;
    const struct rh_snapshot *snapshot;
};
struct rh_allocator {
    void *cookie;
    void *(*alloc)(void *, uint32_t);
    void (*free)(void *, void *);
};
struct rh_mapping_view rh_rules_view(const struct rh_rule *, uint32_t);
struct rh_mapping_view rh_snapshot_view(const struct rh_snapshot *);
/* Snapshot contents are validated by their builder; legacy rules are fully
 * validated here. Only builder-produced immutable snapshots may be used. */
int rh_validate_view(const struct rh_mapping_view *);
typedef int (*rh_open_fn)(void *,const char *,int);
struct rh_state {
    struct rh_rule *rules; uint32_t count, redirected, fallback;
    rh_open_fn original; void *driver; int installed;
    uint32_t rules_capacity;
};
int rh_validate_rules(const struct rh_rule *, uint32_t);
/* Legacy state backing is caller-owned and must have sufficient capacity.
 * Input/staging storage must not overlap the live backing array. */
int rh_configure(struct rh_state *,const struct rh_rule *,uint32_t);
int rh_open(struct rh_state *,void *,const char *,int);
int rh_posix_open(struct rh_state *,void *,const char *,int);
int rh_posix_open_view(struct rh_state *, const struct rh_mapping_view *,
                       void *, const char *, int);
int rh_reinstall_posix(struct rh_state *, void *, rh_open_fn *, rh_open_fn, rh_open_fn);
int rh_install(struct rh_state *,void *,rh_open_fn *,rh_open_fn);
int rh_resolve(const struct rh_state *, const char *, char out[RH_PATH]);
int rh_resolve_view(const struct rh_mapping_view *, const char *, char out[RH_PATH]);
/* TSV destinations are themes/... relative to app files (or @system).
 * Sources may also be validated @quickapp-icon/package declarations; they must
 * be materialized on the UI owner before use as native file mappings.
 * Parsed rules store target-specific absolute native destinations. */
int rh_parse_config(const char *, uint32_t, struct rh_rule *, uint32_t, uint32_t *);
/* Caller owns staging and text buffers; neither may overlap live state.
 * Reader returns bytes read, zero EOF, or a negative error. */
#define RH_CONFIG_BYTES 32768u
typedef int (*rh_read_fn)(void *, void *, uint32_t);
int rh_read_staged_config(rh_read_fn, void *, char *, uint32_t,
                          struct rh_rule *, uint32_t *);
int rh_read_config(struct rh_state *, rh_read_fn, void *, char *, uint32_t,
                   struct rh_rule *);
/* Builders leave *out unchanged on failure; empty success sets it to NULL.
 * parse_snapshot requires caller text to remain immutable through return.
 * -5: reader failure/malformed return, -6: input size overflow, -7: OOM.
 * read_snapshot uses a temporary RH_CONFIG_BYTES buffer, not a rule array. */
int rh_parse_snapshot(const char *, uint32_t, const struct rh_allocator *,
                       struct rh_snapshot **);
int rh_read_snapshot(rh_read_fn, void *, const struct rh_allocator *,
                      struct rh_snapshot **);
void rh_free_snapshot(const struct rh_allocator *, struct rh_snapshot *);
int rh_snapshot_equal(const struct rh_snapshot *, const struct rh_snapshot *);
int rh_snapshot_has_quickapps(const struct rh_snapshot *);
/* UI-owner resolver: 1 = copied native BIN path, 0 = absent/uninitialized,
 * negative = failure. NULL projects only ordinary rules (startup, no UI calls).
 * Declarations remain caller-owned. Expanded key conflicts reject atomically.
 * -8 = lookup failure, -9 = unsupported target/source. */
typedef int (*rh_quickapp_resolver)(void *, const char *, char out[RH_PATH]);
int rh_materialize_snapshot(const struct rh_snapshot *, rh_quickapp_resolver,
                            void *, const struct rh_allocator *, struct rh_snapshot **);
#endif
