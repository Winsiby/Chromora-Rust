#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <setjmp.h>
#include <limits.h>
#include "jpeglib.h"

struct decode_state {
    struct jpeg_decompress_struct jpeg;
    struct jpeg_error_mgr error;
    jmp_buf jump;
    unsigned char *pixels;
    char *message;
    size_t message_size;
};

static void fail(j_common_ptr jpeg) {
    struct decode_state *state = (struct decode_state *)jpeg->client_data;
    char message[JMSG_LENGTH_MAX];
    (*jpeg->err->format_message)(jpeg, message);
    snprintf(state->message, state->message_size, "%s", message);
    longjmp(state->jump, 1);
}

static void emit_message(j_common_ptr jpeg, int level) {
    /* Refuse truncated/corrupt data instead of accepting libjpeg's repaired pixels. */
    if (level < 0) fail(jpeg);
}

void chromora_jpeg_free(unsigned char *pixels) { free(pixels); }

int chromora_jpeg_decode(const unsigned char *input, size_t length,
    unsigned char **pixels, unsigned int *width, unsigned int *height,
    char *message, size_t message_size) {
    struct decode_state *state;
    JSAMPARRAY row;
    size_t count;
    int cmyk, inverted;
    *pixels = NULL;
    *width = *height = 0;
    if (!input || !length || length > ULONG_MAX) {
        snprintf(message, message_size, "Invalid JPEG input size");
        return 1;
    }
    state = (struct decode_state *)calloc(1, sizeof(*state));
    if (!state) {
        snprintf(message, message_size, "Cannot allocate JPEG decoder");
        return 1;
    }
    state->message = message;
    state->message_size = message_size;
    state->jpeg.err = jpeg_std_error(&state->error);
    state->error.error_exit = fail;
    state->error.emit_message = emit_message;
    state->jpeg.client_data = state;
    /* All mutable cleanup state lives on the heap, including across longjmp. */
    if (setjmp(state->jump)) {
        jpeg_destroy_decompress(&state->jpeg);
        free(state->pixels);
        free(state);
        return 1;
    }
    jpeg_create_decompress(&state->jpeg);
    jpeg_mem_src(&state->jpeg, input, (unsigned long)length);
    jpeg_read_header(&state->jpeg, TRUE);
    cmyk = state->jpeg.jpeg_color_space == JCS_CMYK || state->jpeg.jpeg_color_space == JCS_YCCK;
    if (state->jpeg.data_precision != 8 || (!cmyk &&
        state->jpeg.jpeg_color_space != JCS_RGB && state->jpeg.jpeg_color_space != JCS_YCbCr &&
        state->jpeg.jpeg_color_space != JCS_GRAYSCALE)) {
        snprintf(message, message_size, "Only 8-bit RGB, YCbCr, grayscale or CMYK JPEG is supported");
        jpeg_destroy_decompress(&state->jpeg);
        free(state);
        return 1;
    }
    /* Apply the same 512 MiB decoded-pixel ceiling as the image reader. */
    count = (size_t)state->jpeg.image_width * state->jpeg.image_height;
    if (!count || count > (512u * 1024u * 1024u) / 3u) {
        snprintf(message, message_size, "JPEG dimensions exceed the decoded pixel limit");
        jpeg_destroy_decompress(&state->jpeg);
        free(state);
        return 1;
    }
    state->jpeg.out_color_space = cmyk ? JCS_CMYK : JCS_RGB;
    state->jpeg.dct_method = JDCT_ISLOW;
    state->jpeg.do_fancy_upsampling = TRUE;
    inverted = state->jpeg.saw_Adobe_marker || state->jpeg.jpeg_color_space == JCS_YCCK;
    jpeg_start_decompress(&state->jpeg);
    state->pixels = (unsigned char *)malloc(count * 3);
    if (!state->pixels) {
        snprintf(message, message_size, "Cannot allocate JPEG pixels");
        jpeg_destroy_decompress(&state->jpeg);
        free(state);
        return 1;
    }
    row = (*state->jpeg.mem->alloc_sarray)((j_common_ptr)&state->jpeg, JPOOL_IMAGE,
        state->jpeg.output_width * state->jpeg.output_components, 1);
    while (state->jpeg.output_scanline < state->jpeg.output_height) {
        unsigned char *destination = state->pixels + (size_t)state->jpeg.output_scanline * state->jpeg.output_width * 3;
        jpeg_read_scanlines(&state->jpeg, row, 1);
        if (cmyk) {
            unsigned int x, channel;
            for (x = 0; x < state->jpeg.output_width; ++x) {
                unsigned int black = inverted ? row[0][x * 4 + 3] : 255 - row[0][x * 4 + 3];
                for (channel = 0; channel < 3; ++channel) {
                    unsigned int color = inverted ? row[0][x * 4 + channel] : 255 - row[0][x * 4 + channel];
                    destination[x * 3 + channel] = (unsigned char)((color * black + 127) / 255);
                }
            }
        } else memcpy(destination, row[0], (size_t)state->jpeg.output_width * 3);
    }
    jpeg_finish_decompress(&state->jpeg);
    *width = state->jpeg.output_width;
    *height = state->jpeg.output_height;
    *pixels = state->pixels;
    jpeg_destroy_decompress(&state->jpeg);
    free(state);
    return 0;
}
