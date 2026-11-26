/* C interface to the Rust CPTV core (src/lib.rs). */
#ifndef CPTV_CORE_H
#define CPTV_CORE_H

#include <stddef.h>
#include <stdint.h>

typedef struct cptv_clip cptv_clip;

typedef struct {
    uint32_t width;        /* output (scaled) frame width */
    uint32_t height;       /* output (scaled) frame height */
    uint32_t frame_count;  /* playable frames (background frame excluded);
                            * 0 = unknown (CPTV_OPEN_STREAM_ONLY only) */
    double   fps;
    uint64_t timestamp_us; /* recording start, us since epoch, 0 if unknown */
    uint32_t streaming;    /* 1: frames decoded on demand; 0: clip held in memory */
} cptv_info;

/* Byte-stream callbacks; must stay valid until cptv_close(). */
typedef struct {
    void *opaque;
    /* bytes read, 0 at end of stream, negative on error */
    ptrdiff_t (*read)(void *opaque, uint8_t *buf, size_t len);
    /* seek to absolute offset; 0 on success */
    int (*seek)(void *opaque, uint64_t offset);
} cptv_io;

enum {
    CPTV_META_TITLE = 0,
    CPTV_META_DATE = 1,
    CPTV_META_DESCRIPTION = 2,
    CPTV_META_DEVICE = 3,
};

int cptv_probe(const uint8_t *buf, size_t len);
/* Streams frames on demand when the header has total_frames; otherwise
 * decodes the whole clip into memory. */
cptv_clip *cptv_open_io(const cptv_io *io, const char *palette,
                        int normalize_clip, uint32_t scale,
                        char *err, size_t err_len);

/* cptv_open_io_flags flags. */
enum {
    /* Decode on demand even when the header has no total_frames, instead of
     * decoding the whole clip on open. frame_count is then 0 (unknown) and
     * cptv_render returns 0 past the last frame. For thumbnails, which only
     * need an early frame. */
    CPTV_OPEN_STREAM_ONLY = 1,
};
cptv_clip *cptv_open_io_flags(const cptv_io *io, const char *palette,
                              int normalize_clip, uint32_t scale, uint32_t flags,
                              char *err, size_t err_len);
void cptv_get_info(const cptv_clip *clip, cptv_info *info);
const char *cptv_get_meta(const cptv_clip *clip, int key);
/* Renders RV32 (native-endian 0xAARRGGBB) pixels. Returns 1 on success,
 * 0 if there is no such frame (end of clip), -1 on error. */
int cptv_render(cptv_clip *clip, uint32_t index, uint8_t *dst, size_t dst_len,
                size_t pitch);
/* As cptv_render, but renders the clip's last frame if it has fewer than
 * index + 1 frames (returns 0 only if it has none). */
int cptv_render_clamped(cptv_clip *clip, uint32_t index, uint8_t *dst,
                        size_t dst_len, size_t pitch);
void cptv_close(cptv_clip *clip);

#endif
