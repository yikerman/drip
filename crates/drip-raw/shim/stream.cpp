/* LibRaw hands its metadata callbacks the file as a C++ stream object,
 * positioned at the reported entry's value. Rust reads through here. */

#include <libraw.h>

/* LibRaw's streams report errors through their return value, never throw. */
extern "C" size_t drip_raw_read(void *stream, void *dst, size_t len) {
  int n = static_cast<LibRaw_abstract_datastream *>(stream)->read(dst, 1, len);
  return n > 0 ? (size_t)n : 0;
}
