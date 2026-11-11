#pragma once
#include <cstddef>

// Stable POD owned by Chromora; LibRaw's ABI stays inside the C++ bridge.
struct ChromoraRawMetadata {
    char make[64];
    char model[64];
    char lens[128];
    char captured_at[20];
    double iso;
    double shutter;
    double aperture;
    double focal_length;
    double equivalent_focal_length;
    unsigned orientation;
};

extern "C" int chromora_decode_raw(const wchar_t *dll_path, const wchar_t *source,
    unsigned short **pixels, unsigned int *width, unsigned int *height,
    ChromoraRawMetadata *metadata, char *error, size_t error_capacity);
extern "C" void chromora_free_raw(unsigned short *pixels);
