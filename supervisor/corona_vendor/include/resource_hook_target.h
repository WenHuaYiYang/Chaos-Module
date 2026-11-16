#ifndef RESOURCE_HOOK_TARGET_H
#define RESOURCE_HOOK_TARGET_H

/* Exact AP-image addresses, not a version-range ABI. Callable addresses carry
 * the Thumb bit. Default remains 4.100.139; .155/.177/.179 require their
 * respective RH_TARGET_* flag; 10 Pro 3.101.043 requires RH_TARGET_1043 and
 * REDMI Watch 6 3.100.043 requires RH_TARGET_P65, with matching Canopus headers.
 * Static evidence and hardware gates:
 * targets/xiaomi-band-11-4.100.139/ui-reload-audit.md,
 * targets/xiaomi-band-11-4.100.155/ui-reload-audit.md, and
 * targets/xiaomi-band-10-pro-3.101.043/ui-reload-audit.md. */
#if ((defined(RH_TARGET_179) && RH_TARGET_179) + \
     (defined(RH_TARGET_177) && RH_TARGET_177) + \
     (defined(RH_TARGET_155) && RH_TARGET_155) + \
     (defined(RH_TARGET_P65) && RH_TARGET_P65) + \
     (defined(RH_TARGET_1043) && RH_TARGET_1043)) > 1
#error "Select exactly one Resource Hook target"
#endif
#if defined(RH_TARGET_P65) && RH_TARGET_P65
#include "resource_hook_target_p65.h"
#include "resource_hook_p65_features.h"
#include "resource_hook_p65_font.h"
#include "resource_hook_p65_calendar.h"
#include "resource_hook_p65_background.h"
#define RH_HAS_FONT_RELOAD 1
#define RH_HAS_NATIVE_CALENDAR 1
#define RH_HAS_QUICKAPP_ICONS 1
#else
#define RH_HAS_FONT_RELOAD 1
#define RH_HAS_NATIVE_CALENDAR 1
#define RH_HAS_QUICKAPP_ICONS 1
#endif
#if defined(RH_TARGET_P65) && RH_TARGET_P65
/* P65 never inherits a Band 11 or P67 private ABI. */
#elif defined(RH_TARGET_179) && RH_TARGET_179
#include "resource_hook_target_179.h"
#elif defined(RH_TARGET_177) && RH_TARGET_177
#include "resource_hook_target_177.h"
#else
#if defined(RH_TARGET_1043) && RH_TARGET_1043
/* Exact Xiaomi Band 10 Pro 3.101.043 identities. */
/* Font callbacks use their exact startup-copied PSRAM identities. */
#define RH_FW_FR_METRICS 0x1c056e15u
#define RH_FW_FR_OUTLINE 0x1c0572a5u
#define RH_FW_FR_GLYPH_RELEASE 0x1c057289u
#define RH_FW_FR_CONTEXT_SLOT 0x20103374u
#define RH_FW_FR_DRAW_SLOT 0x201032a4u
#define RH_FW_FR_VECTOR_SLOT 0x2013eae0u
#define RH_FW_FR_ALLOC 0x0c16daa8u
#define RH_FW_FR_FREE 0x0c16dae4u
#define RH_FW_FR_OUTLINE_EVENT 0x1c046d09u
#define RH_FW_FR_FACE_COMPARE 0x1c05682du
#define RH_FW_FR_FACE_CREATE 0x1c056719u
#define RH_FW_FR_FACE_DESTROY 0x1c0566edu
#define RH_FW_FR_METRICS_COMPARE 0x1c056f05u
#define RH_FW_FR_METRICS_CREATE 0x1c056d25u
#define RH_FW_FR_METRICS_DESTROY 0x1c056d19u
#define RH_FW_FR_OUTLINE_COMPARE 0x1c057259u
#define RH_FW_FR_OUTLINE_CREATE 0x1c057635u
#define RH_FW_FR_OUTLINE_DESTROY 0x1c057331u
#define RH_FW_FR_INITIALIZED_SLOT 0x20103178u
#define RH_FW_FR_STYLE_ENABLED_SLOT 0x2010319cu
#define RH_FW_FR_DISPLAY_LIST 0x2010317cu
#define RH_FW_FR_VG_DISPATCH 0x1c045361u
#define RH_FW_FR_GLYPH_PENDING_FREE 0x1c046cfdu
#define RH_FW_FR_IMAGE_PENDING_FREE 0x1c04d259u
#define RH_FW_FR_GRADIENT_PENDING_FREE 0x1c0493bdu
#define RH_FW_FR_VG_SLOT 0x2013eadcu
#define RH_FW_FR_SW_DISPATCH 0x1c03debdu
#define RH_FW_FR_CACHE_INIT 0x0c272d38u
#define RH_FW_FR_DROP_FACE_ID 0x0c163be8u
#define RH_FW_FR_CACHE_ACQUIRE 0x0c1666d0u
#define RH_FW_FR_CACHE_ACQUIRE_CREATE 0x0c166740u
#define RH_FW_FR_FT_MUL_FIX 0x0c33d8bcu
#define RH_FW_FR_LIST_REMOVE 0x0c169ce8u
#define RH_FW_FR_FACE_SIZE 24u
#define RH_FW_FR_VECTOR_DROP_SECOND_ARG 1
#define RH_FW_VECTOR_CLASS 0x2cdba868u
#define RH_FW_VECTOR_COMPARE 0x0ca657edu
#define RH_FW_VECTOR_DESTROY 0x0ca6581du
#define RH_FW_VECTOR_DROP 0x0ca65c25u
#define RH_FW_FONT_SET_PIXEL_SIZE 0x0c33ef59u

#define RH_CALENDAR_BACKGROUND "/resource/app/perpetual_calendar/calendar_background_icon.bin"
#define RH_CALENDAR_OUTPUT "/resource/app/perpetual_calendar/launcher.bin"
#define RH_CALENDAR_APP_ID "com.xiaomi.miwear.perpetual_calendar"
#define RH_FW_CALENDAR_LOOKUP         0x0ca69935u
#define RH_FW_CALENDAR_LOOKUP_OTHER   0x0ca6996du
#define RH_FW_CALENDAR_SIGNAL         0x0c4efde9u
#define RH_FW_CALENDAR_NOTIFY         0x0ca6a005u
#define RH_FW_CALENDAR_VTABLE_SLOT    0x200eb658u
#define RH_FW_CALENDAR_VTABLE         0x2cdbb054u
#define RH_FW_CALENDAR_LOOKUP_NAME    0x0ca69e81u
#define RH_FW_CALENDAR_LOOKUP_NAME_OTHER 0x0ca69e55u
#define RH_FW_CALENDAR_DISPATCH       0x0ca69aa1u
#define RH_FW_IMAGE_OBJECT_CLASS      0x2cce61ecu
#define RH_FW_CACHE_DROP              0x0c1667ddu
#define RH_FW_IMAGE_CACHE_CLASS       0x2cce56f4u
#define RH_FW_HEADER_CACHE_CLASS      0x2cce571cu
#define RH_FW_FONT_REMOVE_PATH        0x0c860399u
#define RH_FW_CACHE_RELEASE           0x0c1666edu

/* Startup-copied executable aliases, as pinned by the current .043 pack:
 * flash .0c17a2f4 -> .1c06ceb4; flash .0c143798 -> .1c036358.
 * Load addresses are not callable after startup relocation. */
#define RH_FW_IMAGE_SET_SRC           0x1c06ceb5u
#define RH_FW_IMAGE_GET_INFO          0x1c036359u
#define RH_FW_OBJECT_TREE_WALK        0x0c13c691u
#define RH_FW_OBJECT_STYLE_GET        0x0c1068a9u
#define RH_FW_OBJECT_STYLE_REFRESH    0x0c1070adu

#define RH_FW_POSIX_DRIVER            0x20103340u
#define RH_FW_POSIX_CACHE_SIZE        0x20103344u
#define RH_FW_POSIX_OPEN_SLOT         0x2010334cu
#define RH_FW_POSIX_OPEN              0x1c057c51u
#define RH_FW_IMAGE_CACHE_SLOT        0x2010329cu
#define RH_FW_HEADER_CACHE_SLOT       0x201032a0u
#define RH_FW_DISPLAY_SLOT            0x2010318cu
#define RH_FW_INVALIDATE_AREA         0x0c105165u
#define RH_FW_DISPLAY_WIDTH           0x0c13cd55u
#define RH_FW_DISPLAY_HEIGHT          0x0c13cd75u
#define RH_FW_UIKIT_SLOT              0x20103174u
#define RH_FW_UIKIT_MANAGER_OFFSET    28u
#define RH_FW_MANAGER_PATH_LIST_HEAD_OFFSET 28u
#define RH_FW_FONT_ADD_PATH           0x0c86037du
#elif defined(RH_TARGET_155) && RH_TARGET_155
/* Exact .155 cache and LVGL image-class identities. */
#define RH_FW_IMAGE_OBJECT_CLASS  0x2ca14ca8u
#define RH_FW_CACHE_DROP          0x0c8b8c9fu
#define RH_FW_IMAGE_CACHE_CLASS   0x2ca168b4u
#define RH_FW_HEADER_CACHE_CLASS  0x2ca16934u
#define RH_FW_FONT_REMOVE_PATH    0x0c904cedu
/* Font transaction identities: individually mapped, not a range delta. */
#define RH_FW_VECTOR_CLASS            0x2ca6ee48u
#define RH_FW_VECTOR_COMPARE          0x0c69feb1u
#define RH_FW_VECTOR_DESTROY          0x0c6a12d5u
#define RH_FW_VECTOR_DROP             0x0c6a1305u
#define RH_FW_FONT_SET_PIXEL_SIZE     0x0c8b8a55u
#define RH_FW_CACHE_RELEASE           0x0c8b9781u
#else
/* Exact .139 cache and LVGL image-class identities, verified in its AP. */
#define RH_FW_IMAGE_OBJECT_CLASS  0x2ca14cb8u
#define RH_FW_CACHE_DROP          0x0c8b8cafu
#define RH_FW_IMAGE_CACHE_CLASS   0x2ca168c4u
#define RH_FW_HEADER_CACHE_CLASS  0x2ca16944u
#define RH_FW_FONT_REMOVE_PATH    0x0c904cfdu
/* Font transaction identities: individually mapped, not a range delta. */
#define RH_FW_VECTOR_CLASS            0x2ca6ee58u
#define RH_FW_VECTOR_COMPARE          0x0c69fec1u
#define RH_FW_VECTOR_DESTROY          0x0c6a12e5u
#define RH_FW_VECTOR_DROP             0x0c6a1315u
#define RH_FW_FONT_SET_PIXEL_SIZE     0x0c8b8a65u
#define RH_FW_CACHE_RELEASE           0x0c8b9791u
#endif

#if !(defined(RH_TARGET_1043) && RH_TARGET_1043)
/* Shared .139/.155 font transaction identities, independently byte-audited. */
#define RH_FW_FR_FACE_SIZE 28u
#define RH_FW_FR_VECTOR_DROP_SECOND_ARG 0
#define RH_FW_FR_METRICS 0x0c396b5du
#define RH_FW_FR_OUTLINE 0x0c3a7c45u
#define RH_FW_FR_GLYPH_RELEASE 0x0c3a0993u
#define RH_FW_FR_CONTEXT_SLOT 0x200bd3ecu
#define RH_FW_FR_DRAW_SLOT 0x200bd318u
#define RH_FW_FR_VECTOR_SLOT 0x200d3280u
#define RH_FW_FR_ALLOC 0x0c3abe20u
#define RH_FW_FR_FREE 0x0c3abe58u
#define RH_FW_FR_OUTLINE_EVENT 0x0c3981d1u
#define RH_FW_FR_FACE_COMPARE 0x0c396785u
#define RH_FW_FR_FACE_CREATE 0x0c3967b5u
#define RH_FW_FR_FACE_DESTROY 0x0c396b25u
#define RH_FW_FR_METRICS_COMPARE 0x0c39fd9bu
#define RH_FW_FR_METRICS_CREATE 0x0c3a5ef1u
#define RH_FW_FR_METRICS_DESTROY 0x0c39fd95u
#define RH_FW_FR_OUTLINE_COMPARE 0x0c39fdcdu
#define RH_FW_FR_OUTLINE_CREATE 0x0c3a8bb9u
#define RH_FW_FR_OUTLINE_DESTROY 0x0c3a09f5u
#define RH_FW_FR_INITIALIZED_SLOT 0x200bd1ecu
#define RH_FW_FR_STYLE_ENABLED_SLOT 0x200bd210u
#define RH_FW_FR_DISPLAY_LIST 0x200bd1f0u
#define RH_FW_FR_VG_DISPATCH 0x0c3913edu
#define RH_FW_FR_GLYPH_PENDING_FREE 0x0c399b63u
#define RH_FW_FR_IMAGE_PENDING_FREE 0x0c395be1u
#define RH_FW_FR_GRADIENT_PENDING_FREE 0x0c395bd1u
#define RH_FW_FR_VG_SLOT 0x200d327cu
#define RH_FW_FR_SW_DISPATCH 0x0c3948adu
#define RH_FW_FR_CACHE_INIT 0x0c3a9e48u
#define RH_FW_FR_DROP_FACE_ID 0x0c39a424u
#define RH_FW_FR_CACHE_ACQUIRE 0x0c3a3860u
#define RH_FW_FR_CACHE_ACQUIRE_CREATE 0x0c3a7b78u
#define RH_FW_FR_FT_MUL_FIX 0x0c424304u
#define RH_FW_FR_LIST_REMOVE 0x0c3a46dcu
/* Native launcher calendar generation/publication have the same entries in
 * both exact APs; snapshot leaves/literals are individually target-specific. */
#define RH_CALENDAR_BACKGROUND "/resource/app/perpetual_calendar/launcher_icon.bin"
#define RH_CALENDAR_OUTPUT "/data/app/perpetual_calendar/calendar_icon.bin"
#define RH_FW_CALENDAR_GENERATE       0x0c5487a5u
#define RH_FW_CALENDAR_PUBLISH        0x0c5486d1u
#define RH_FW_CALENDAR_LAUNCHER_SLOT  0x200c6010u
/* Get-info, tree walk and style APIs are byte-identical at these Thumb
 * addresses in both exact APs. The setter entry is also shared but its .139
 * control flow is independently audited. The reload probe executes both APs. */
#define RH_FW_IMAGE_SET_SRC       0x0c3b2c29u
#define RH_FW_IMAGE_GET_INFO      0x0c38e4e5u
#define RH_FW_OBJECT_TREE_WALK    0x0c380575u
#define RH_FW_OBJECT_STYLE_GET    0x0c382621u
#define RH_FW_OBJECT_STYLE_REFRESH 0x0c38525du

/* Independently checked in the .155 AP; unchanged from .139. */
#define RH_FW_POSIX_DRIVER        0x200bd3b8u
#define RH_FW_POSIX_CACHE_SIZE    0x200bd3bcu
#define RH_FW_POSIX_OPEN_SLOT     0x200bd3c4u
#define RH_FW_POSIX_OPEN          0x0c3a6195u
#define RH_FW_IMAGE_CACHE_SLOT    0x200bd310u
#define RH_FW_HEADER_CACHE_SLOT   0x200bd314u
#define RH_FW_DISPLAY_SLOT        0x200bd200u
#define RH_FW_INVALIDATE_AREA     0x0c382429u
#define RH_FW_DISPLAY_WIDTH       0x0c380695u
#define RH_FW_DISPLAY_HEIGHT      0x0c3806b5u
#define RH_FW_UIKIT_SLOT          0x200bd1e8u
#define RH_FW_UIKIT_MANAGER_OFFSET 20u
#define RH_FW_MANAGER_PATH_LIST_HEAD_OFFSET 4u
#define RH_FW_FONT_ADD_PATH       0x0c4924e1u
#define RH_FW_FONT_RESOLVE_PATH   0x0c490eddu
#endif

#endif /* .177/.179 versus legacy targets */
/* Feature capability is exact-target based; never enable on .043. */
#if defined(RH_TARGET_P65) && RH_TARGET_P65
#define RH_Q66_BACKGROUND 0
#define RH_Q66_CALENDAR 0
#elif defined(RH_TARGET_179) && RH_TARGET_179
#define RH_Q66_BACKGROUND 1
#define RH_Q66_CALENDAR 1
#include "resource_hook_q66_179.h"
#elif defined(RH_TARGET_177) && RH_TARGET_177
#define RH_Q66_BACKGROUND 1
#define RH_Q66_CALENDAR 1
#include "resource_hook_q66_177.h"
#elif defined(RH_TARGET_155) && RH_TARGET_155
#define RH_Q66_BACKGROUND 1
#define RH_Q66_CALENDAR 1
#include "resource_hook_q66_155.h"
#elif !(defined(RH_TARGET_1043) && RH_TARGET_1043)
#define RH_Q66_BACKGROUND 1
#define RH_Q66_CALENDAR 1
#include "resource_hook_q66_139.h"
#else
#define RH_Q66_BACKGROUND 0
#define RH_Q66_CALENDAR 0
#endif
#endif
