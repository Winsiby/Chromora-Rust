#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <ctime>
#include "raw_bridge.h"
#include "libraw/libraw.h"

static_assert(sizeof(ChromoraRawMetadata) == 328, "Chromora metadata ABI changed");

static void copy_text(char *destination, size_t capacity, const char *source, size_t source_capacity) {
    size_t length = 0;
    while (length < source_capacity && source[length] != 0 && length + 1 < capacity) ++length;
    std::memcpy(destination, source, length);
    destination[length] = 0;
}

static void copy_metadata(const libraw_data_t *raw, ChromoraRawMetadata *metadata) {
    copy_text(metadata->make, sizeof metadata->make, raw->idata.make, sizeof raw->idata.make);
    copy_text(metadata->model, sizeof metadata->model, raw->idata.model, sizeof raw->idata.model);
    copy_text(metadata->lens, sizeof metadata->lens, raw->lens.Lens, sizeof raw->lens.Lens);
    metadata->iso = raw->other.iso_speed;
    metadata->shutter = raw->other.shutter;
    metadata->aperture = raw->other.aperture;
    metadata->focal_length = raw->other.focal_len;
    metadata->equivalent_focal_length = raw->lens.FocalLengthIn35mmFormat;
    // Invert LibRaw's mktime conversion to recover the recorded wall clock;
    // this string makes no timezone or UTC claim.
    if (raw->other.timestamp > 0) {
        struct tm recorded {};
        if (localtime_s(&recorded, &raw->other.timestamp) == 0)
            std::strftime(metadata->captured_at, sizeof metadata->captured_at, "%Y:%m:%d %H:%M:%S", &recorded);
    }
    // A default flip=0 does not prove the file contained an EXIF orientation.
    constexpr unsigned orientation[] = {0, 2, 4, 3, 5, 8, 6, 7};
    if (raw->sizes.flip > 0 && raw->sizes.flip < 8) metadata->orientation = orientation[raw->sizes.flip];
}

// The only ABI touching LibRaw structs is compiled against the bundled DLL's
// exact headers. Rust receives an owned RGB16 buffer, never a LibRaw struct.
extern "C" int chromora_decode_raw(const wchar_t *dll_path, const wchar_t *source,
    unsigned short **pixels, unsigned int *width, unsigned int *height,
    ChromoraRawMetadata *metadata, char *error, size_t error_capacity) {
    *pixels = nullptr;
    *width = *height = 0;
    std::memset(metadata, 0, sizeof *metadata);
    auto fail = [&](const char *message) { std::snprintf(error, error_capacity, "%s", message); return -1; };
    HMODULE module = LoadLibraryExW(dll_path, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module) return fail("Cannot load bundled LibRaw 0.22.3 DLL (or a required VC runtime).");
    struct ModuleOwner { HMODULE module; ~ModuleOwner() { FreeLibrary(module); } } owner{module};
#define LOAD_PROC(name) auto name##_p = reinterpret_cast<decltype(&name)>(GetProcAddress(module, #name)); if (!name##_p) return fail("Missing LibRaw API: " #name)
    LOAD_PROC(libraw_versionNumber);
    LOAD_PROC(libraw_init);
    LOAD_PROC(libraw_open_wfile);
    LOAD_PROC(libraw_unpack);
    LOAD_PROC(libraw_dcraw_process);
    LOAD_PROC(libraw_dcraw_make_mem_image);
    LOAD_PROC(libraw_dcraw_clear_mem);
    LOAD_PROC(libraw_close);
    LOAD_PROC(libraw_strerror);
#undef LOAD_PROC
    if (libraw_versionNumber_p() != LIBRAW_VERSION) return fail("LibRaw DLL/header version mismatch; expected 0.22.3.");
    libraw_data_t *raw = libraw_init_p(0);
    if (!raw) return fail("LibRaw initialization failed.");
    struct RawOwner { libraw_data_t *raw; decltype(&libraw_close) close; ~RawOwner() { close(raw); } } raw_owner{raw, libraw_close_p};
    raw->params.use_camera_wb = 1;
    raw->params.no_auto_bright = 1;
    raw->params.output_bps = 16;
    raw->params.output_color = 1; // sRGB, matching the former rawpy path.
    raw->params.gamm[0] = 1.0 / 2.4;
    raw->params.gamm[1] = 12.92;
    int status = libraw_open_wfile_p(raw, source);
    if (!status) copy_metadata(raw, metadata);
    if (!status) status = libraw_unpack_p(raw);
    if (!status) status = libraw_dcraw_process_p(raw);
    if (status) return fail(libraw_strerror_p(status));
    int memory_status = 0;
    libraw_processed_image_t *image = libraw_dcraw_make_mem_image_p(raw, &memory_status);
    if (!image) return fail(libraw_strerror_p(memory_status));
    struct ImageOwner { libraw_processed_image_t *image; decltype(&libraw_dcraw_clear_mem) clear; ~ImageOwner() { clear(image); } } image_owner{image, libraw_dcraw_clear_mem_p};
    if (image->type != LIBRAW_IMAGE_BITMAP || image->colors != 3 || image->bits != 16 || image->width == 0 || image->height == 0)
        return fail("LibRaw returned an unsupported bitmap layout.");
    const size_t sample_count = size_t(image->width) * image->height * 3;
    if (sample_count > std::numeric_limits<size_t>::max() / sizeof(unsigned short)
        || sample_count * sizeof(unsigned short) != image->data_size)
        return fail("Invalid LibRaw RGB16 buffer size.");
    auto result = static_cast<unsigned short *>(std::malloc(image->data_size));
    if (!result) return fail("Insufficient memory for RAW RGB16 pixels.");
    std::memcpy(result, image->data, image->data_size);
    *pixels = result;
    *width = image->width;
    *height = image->height;
    return 0;
}

extern "C" void chromora_free_raw(unsigned short *pixels) { std::free(pixels); }
