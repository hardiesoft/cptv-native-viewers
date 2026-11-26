/*****************************************************************************
 * cptv_demux.c: VLC demuxer for Cacophony Project thermal video (CPTV) files
 *
 * Thin VLC glue around the Rust decoder in src/lib.rs, which reads the file
 * through callbacks on a VLC stream. Clips whose header gives the frame count
 * are decoded on demand (seeking back rewinds and re-decodes); others are
 * decoded into memory on open. Each frame is handed to VLC as a raw RV32
 * picture.
 *****************************************************************************/

#define MODULE_NAME cptv
#define MODULE_STRING "cptv"
#ifndef __PLUGIN__
# define __PLUGIN__
#endif

#include <stdlib.h>
#include <string.h>

#include <vlc_common.h>
#include <vlc_plugin.h>

/* Some distributions (e.g. Debian/Ubuntu's 64-bit time_t transition) change
 * the plugin ABI tag that VLC looks for; the Makefile detects it from the
 * installed libvlccore and passes it in. */
#ifdef CPTV_MODULE_SYMBOL
# undef MODULE_SYMBOL
# define MODULE_SYMBOL CPTV_MODULE_SYMBOL
#endif
#include <vlc_demux.h>
#include <vlc_meta.h>
#include <vlc_modules.h>

#include "cptv_core.h"

/* Normally provided by VLC's in-tree config.h / vlc_fixups.h. */
#ifndef N_
# define N_(str) (str)
#endif

#define PROBE_BYTES 4096

static int  Open (vlc_object_t *);
static void Close(vlc_object_t *);

static const char *const palette_values[] = {
    "viridis", "inferno", "magma", "plasma", "turbo", "ironbow", "grey",
};
static const char *const palette_texts[] = {
    "Viridis", "Inferno", "Magma", "Plasma", "Turbo", "Ironbow",
    "Greyscale (white hot)",
};

#define PALETTE_TEXT N_("Colour palette")
#define PALETTE_LONGTEXT N_("Colour map used to display thermal values.")
#define NORM_TEXT N_("Normalise over whole clip")
#define NORM_LONGTEXT N_("Use one brightness range for the whole clip " \
    "(from the min/max values in the file header when present, otherwise " \
    "computed over all frames). Turn off to stretch each frame individually " \
    "for more contrast but less stable brightness.")
#define SCALE_TEXT N_("Upscale factor")
#define SCALE_LONGTEXT N_("CPTV frames are 160x120; they are smoothly " \
    "upscaled by this factor before display.")

vlc_module_begin ()
    set_shortname ("CPTV")
    set_description (N_("Cacophony Project thermal video (CPTV) demuxer"))
    set_category (CAT_INPUT)
    set_subcategory (SUBCAT_INPUT_DEMUX)
    set_capability ("demux", 155)
    set_callbacks (Open, Close)
    add_shortcut ("cptv")
    add_string ("cptv-palette", "viridis", PALETTE_TEXT, PALETTE_LONGTEXT, false)
        change_string_list (palette_values, palette_texts)
    add_bool ("cptv-normalize-clip", true, NORM_TEXT, NORM_LONGTEXT, false)
    add_integer_with_range ("cptv-scale", 4, 1, 16, SCALE_TEXT, SCALE_LONGTEXT, false)
vlc_module_end ()

struct demux_sys_t
{
    cptv_clip  *clip;
    es_out_id_t *es;
    cptv_info   info;
    uint32_t    frame;      /* next frame to send */
    size_t      frame_size; /* bytes per RV32 picture */
};

static vlc_tick_t FrameTime(const demux_sys_t *sys, uint32_t frame)
{
    return (vlc_tick_t)((double)frame * CLOCK_FREQ / sys->info.fps);
}

static int Demux(demux_t *demux)
{
    demux_sys_t *sys = demux->p_sys;

    if (sys->frame >= sys->info.frame_count)
        return VLC_DEMUXER_EOF;

    vlc_tick_t pts = VLC_TS_0 + FrameTime(sys, sys->frame);
    es_out_SetPCR(demux->out, pts);

    block_t *block = block_Alloc(sys->frame_size);
    if (unlikely(block == NULL))
        return VLC_DEMUXER_EGENERIC;

    int ret = cptv_render(sys->clip, sys->frame, block->p_buffer,
                          sys->frame_size, sys->info.width * 4);
    if (ret <= 0)
    {
        /* With streaming, the header's frame count can overstate what is
         * actually in a truncated file; treat that as the end of the clip. */
        block_Release(block);
        if (ret < 0)
            msg_Err(demux, "failed to render frame %u", sys->frame);
        else if (sys->frame + 1 < sys->info.frame_count)
            msg_Warn(demux, "clip ended at frame %u of %u", sys->frame,
                     sys->info.frame_count);
        return ret < 0 ? VLC_DEMUXER_EGENERIC : VLC_DEMUXER_EOF;
    }
    block->i_pts = block->i_dts = pts;
    block->i_length = FrameTime(sys, sys->frame + 1) - FrameTime(sys, sys->frame);
    block->i_flags |= BLOCK_FLAG_TYPE_I;
    es_out_Send(demux->out, sys->es, block);

    sys->frame++;
    return VLC_DEMUXER_SUCCESS;
}

static void SeekToFrame(demux_t *demux, int64_t frame)
{
    demux_sys_t *sys = demux->p_sys;
    if (frame < 0)
        frame = 0;
    if (frame > sys->info.frame_count)
        frame = sys->info.frame_count;
    sys->frame = (uint32_t)frame;
}

static int Control(demux_t *demux, int query, va_list args)
{
    demux_sys_t *sys = demux->p_sys;
    const uint32_t n = sys->info.frame_count;

    switch (query)
    {
        case DEMUX_CAN_SEEK:
        case DEMUX_CAN_PAUSE:
        case DEMUX_CAN_CONTROL_PACE:
            *va_arg(args, bool *) = true;
            return VLC_SUCCESS;

        case DEMUX_SET_PAUSE_STATE:
            return VLC_SUCCESS;

        case DEMUX_GET_PTS_DELAY:
            *va_arg(args, int64_t *) =
                INT64_C(1000) * var_InheritInteger(demux, "file-caching");
            return VLC_SUCCESS;

        case DEMUX_GET_LENGTH:
            *va_arg(args, int64_t *) = FrameTime(sys, n);
            return VLC_SUCCESS;

        case DEMUX_GET_TIME:
            *va_arg(args, int64_t *) = FrameTime(sys, sys->frame);
            return VLC_SUCCESS;

        case DEMUX_SET_TIME:
        {
            int64_t t = va_arg(args, int64_t);
            SeekToFrame(demux, (int64_t)((double)t * sys->info.fps / CLOCK_FREQ + 0.5));
            return VLC_SUCCESS;
        }

        case DEMUX_GET_POSITION:
            *va_arg(args, double *) = n ? (double)sys->frame / n : 0.0;
            return VLC_SUCCESS;

        case DEMUX_SET_POSITION:
        {
            double pos = va_arg(args, double);
            SeekToFrame(demux, (int64_t)(pos * n + 0.5));
            return VLC_SUCCESS;
        }

        case DEMUX_GET_FPS:
            *va_arg(args, double *) = sys->info.fps;
            return VLC_SUCCESS;

        case DEMUX_GET_META:
        {
            vlc_meta_t *meta = va_arg(args, vlc_meta_t *);
            const char *s;
            if ((s = cptv_get_meta(sys->clip, CPTV_META_TITLE)) != NULL)
                vlc_meta_SetTitle(meta, s);
            if ((s = cptv_get_meta(sys->clip, CPTV_META_DATE)) != NULL)
                vlc_meta_SetDate(meta, s);
            if ((s = cptv_get_meta(sys->clip, CPTV_META_DESCRIPTION)) != NULL)
                vlc_meta_SetDescription(meta, s);
            if ((s = cptv_get_meta(sys->clip, CPTV_META_DEVICE)) != NULL)
                vlc_meta_SetArtist(meta, s);
            return VLC_SUCCESS;
        }

        default:
            return VLC_EGENERIC;
    }
}

/* cptv_io callbacks over a VLC stream. */
static ptrdiff_t IoRead(void *opaque, uint8_t *buf, size_t len)
{
    return vlc_stream_Read((stream_t *)opaque, buf, len);
}

static int IoSeek(void *opaque, uint64_t offset)
{
    return vlc_stream_Seek((stream_t *)opaque, offset) == VLC_SUCCESS ? 0 : -1;
}

static int Open(vlc_object_t *obj)
{
    demux_t *demux = (demux_t *)obj;

    const uint8_t *peek;
    ssize_t peeked = vlc_stream_Peek(demux->s, &peek, PROBE_BYTES);
    if (peeked < 6 || !cptv_probe(peek, (size_t)peeked))
        return VLC_EGENERIC;

    /* CPTV files are multi-member gzip streams (header and frames are
     * separate members). VLC's "inflate" stream filter stops after the first
     * member, so if it was inserted, read the compressed bytes from beneath
     * it instead and let the Rust decoder inflate them. */
    stream_t *src = demux->s;
    for (stream_t *s = demux->s; s != NULL; s = s->p_source)
    {
        if (s->p_module != NULL && s->p_source != NULL
         && strcmp(module_get_object(s->p_module), "inflate") == 0)
        {
            src = s->p_source;
            break;
        }
    }
    const cptv_io io = { .opaque = src, .read = IoRead, .seek = IoSeek };

    char *palette = var_InheritString(demux, "cptv-palette");
    bool norm_clip = var_InheritBool(demux, "cptv-normalize-clip");
    int64_t scale = var_InheritInteger(demux, "cptv-scale");
    if (scale < 1) scale = 1;
    if (scale > 16) scale = 16;

    char err[256] = "";
    cptv_clip *clip = cptv_open_io(&io, palette, norm_clip, (uint32_t)scale,
                                   err, sizeof(err));
    free(palette);
    if (clip == NULL)
    {
        msg_Err(demux, "cannot decode CPTV file: %s", err);
        return VLC_EGENERIC;
    }

    demux_sys_t *sys = calloc(1, sizeof(*sys));
    if (unlikely(sys == NULL))
    {
        cptv_close(clip);
        return VLC_ENOMEM;
    }
    sys->clip = clip;
    cptv_get_info(clip, &sys->info);
    if (!(sys->info.fps > 0.0))
        sys->info.fps = 9.0;
    sys->frame_size = (size_t)sys->info.width * sys->info.height * 4;

    es_format_t fmt;
    es_format_Init(&fmt, VIDEO_ES, VLC_CODEC_RGB32);
    video_format_Setup(&fmt.video, VLC_CODEC_RGB32,
                       sys->info.width, sys->info.height,
                       sys->info.width, sys->info.height, 1, 1);
    fmt.video.i_rmask = 0x00ff0000;
    fmt.video.i_gmask = 0x0000ff00;
    fmt.video.i_bmask = 0x000000ff;
    vlc_ureduce(&fmt.video.i_frame_rate, &fmt.video.i_frame_rate_base,
                (uint64_t)(sys->info.fps * 1000.0 + 0.5), 1000, 0);
    fmt.psz_description = strdup("Thermal (CPTV)");

    sys->es = es_out_Add(demux->out, &fmt);
    es_format_Clean(&fmt);
    if (sys->es == NULL)
    {
        cptv_close(clip);
        free(sys);
        return VLC_EGENERIC;
    }

    msg_Dbg(demux, "CPTV: %u frames at %.2f fps, output %ux%u, %s%s",
            sys->info.frame_count, sys->info.fps, sys->info.width, sys->info.height,
            sys->info.streaming ? "streaming" : "decoded into memory",
            src != demux->s ? " (bypassing inflate filter)" : "");

    demux->p_sys = sys;
    demux->pf_demux = Demux;
    demux->pf_control = Control;
    return VLC_SUCCESS;
}

static void Close(vlc_object_t *obj)
{
    demux_t *demux = (demux_t *)obj;
    demux_sys_t *sys = demux->p_sys;
    cptv_close(sys->clip);
    free(sys);
}
