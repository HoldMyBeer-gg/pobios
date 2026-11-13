// The Rust tree host's C interface. Source: crates/pobng-ios/src/lib.rs.
#ifndef POB_BRIDGING_HEADER_H
#define POB_BRIDGING_HEADER_H

#include <stdint.h>
#include <stdbool.h>
#include <stddef.h>

// uniffi's generated interface, which the Swift in Generated/ needs.
#include "Generated/pobng_ffiFFI.h"

/// Start the tree canvas on a CAMetalLayer. Returns NULL if GPU setup failed.
void *pobng_start(void *layer, const char *support_dir, const char *data_dir,
                  uint32_t width, uint32_t height, float scale);

/// Draw one frame. Returns milliseconds until the next frame is wanted;
/// UINT32_MAX means nothing is animating and the view can idle.
uint32_t pobng_frame(void *handle);

/// The surface resized. Called when a resize *finishes*, not during the drag.
void pobng_resize(void *handle, uint32_t width, uint32_t height, float scale);

/// A touch began (0), moved (1), ended (2) or was cancelled (3), in points.
void pobng_touch(void *handle, uint32_t phase, float x, float y);

/// A two-finger scroll, in points.
void pobng_scroll(void *handle, float dx, float dy);

/// A pinch, as a scale factor relative to the previous event.
void pobng_zoom(void *handle, float factor);

/// Highlight nodes matching a search. An empty list clears it.
void pobng_set_search(void *handle, const uint64_t *ids, size_t count,
                      bool searching);

/// Centre the view on a node and pin its tooltip.
void pobng_focus_node(void *handle, uint64_t id);
uint64_t pobng_pinned_node(void *handle);
void pobng_unpin(void *handle);

/// Colour nodes by a ranking. Values are 0..1; an empty list clears it.
void pobng_set_power(void *handle, const uint64_t *ids, const float *values,
                     size_t count);

/// Free the canvas, saving settings on the way out.
void pobng_stop(void *handle);

#endif
