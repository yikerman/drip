/* The only code that touches LibRaw. Rust never mirrors LibRaw's own structs,
 * whose layout changes between releases; it sees only drip_raw_info below,
 * which this file fills by name against the headers it is compiled with. */

#include <libraw.h>
#include <stdlib.h>
#include <string.h>

#define DRIP_CBLACK_SIZE 4104
#define DRIP_NOT_BAYER (-100000)

_Static_assert(LIBRAW_CBLACK_SIZE <= DRIP_CBLACK_SIZE, "cblack does not fit");

typedef struct {
  int width, height; /* visible area */
  int cfa[2][2];     /* LibRaw color index (0 R, 1 G, 2 B, 3 G2) per row%2, col%2 */
  unsigned black, maximum;
  unsigned cblack[DRIP_CBLACK_SIZE];
  float cam_xyz[4][3]; /* XYZ (D65) to camera */
  float cam_mul[4];    /* as-shot white balance multipliers */
  char make[64], model[64];
  float iso_speed, shutter, aperture, focal_len;
  long long timestamp;
} drip_raw_info;

static int is_bayer(libraw_data_t *lr) {
  return lr->rawdata.raw_image && lr->idata.filters > 1000 && lr->idata.colors == 3 &&
         lr->sizes.top_margin + lr->sizes.height <= lr->sizes.raw_height &&
         lr->sizes.left_margin + lr->sizes.width <= lr->sizes.raw_width;
}

int drip_raw_open(const char *path, void **handle, drip_raw_info *info) {
  libraw_data_t *lr = libraw_init(0);
  if (!lr)
    return LIBRAW_UNSUFFICIENT_MEMORY;
  int err = libraw_open_file(lr, path);
  if (!err)
    err = libraw_unpack(lr);
  if (!err && !is_bayer(lr))
    err = DRIP_NOT_BAYER;
  if (err) {
    libraw_close(lr);
    return err;
  }
  info->width = lr->sizes.width;
  info->height = lr->sizes.height;
  for (int r = 0; r < 2; r++)
    for (int c = 0; c < 2; c++)
      info->cfa[r][c] = libraw_COLOR(lr, r, c);
  info->black = lr->color.black;
  info->maximum = lr->color.maximum;
  memcpy(info->cblack, lr->color.cblack, sizeof lr->color.cblack);
  memcpy(info->cam_xyz, lr->color.cam_xyz, sizeof info->cam_xyz);
  memcpy(info->cam_mul, lr->color.cam_mul, sizeof info->cam_mul);
  memcpy(info->make, lr->idata.normalized_make, sizeof info->make);
  memcpy(info->model, lr->idata.normalized_model, sizeof info->model);
  info->make[sizeof info->make - 1] = info->model[sizeof info->model - 1] = 0;
  info->iso_speed = lr->other.iso_speed;
  info->shutter = lr->other.shutter;
  info->aperture = lr->other.aperture;
  info->focal_len = lr->other.focal_len;
  info->timestamp = lr->other.timestamp;
  *handle = lr;
  return 0;
}

/* Copies the visible area into dst, which holds width * height values. */
void drip_raw_copy(void *handle, unsigned short *dst) {
  libraw_data_t *lr = handle;
  const libraw_image_sizes_t *s = &lr->sizes;
  for (int row = 0; row < s->height; row++)
    memcpy(dst + (size_t)row * s->width,
           lr->rawdata.raw_image + (size_t)(row + s->top_margin) * (s->raw_pitch / 2) + s->left_margin,
           (size_t)s->width * sizeof *dst);
}

void drip_raw_close(void *handle) { libraw_close(handle); }

const char *drip_raw_strerror(int err) {
  return err == DRIP_NOT_BAYER ? "not a 3-color Bayer raw" : libraw_strerror(err);
}

/* LibRaw's own pipeline in half-size mode (2x2 binning, greens averaged), as-shot
 * white balance, no highlight recovery, linear Rec.2020, 16 bit, unrotated.
 * On success *rgb is a malloc'd width * height * 3 buffer the caller frees. */
int drip_raw_reference(const char *path, int *width, int *height, unsigned short **rgb) {
  libraw_data_t *lr = libraw_init(0);
  if (!lr)
    return LIBRAW_UNSUFFICIENT_MEMORY;
  lr->params.half_size = 1;
  lr->params.use_camera_wb = 1;
  lr->params.output_color = 8; /* Rec. 2020 */
  lr->params.gamm[0] = lr->params.gamm[1] = 1;
  lr->params.no_auto_bright = 1;
  lr->params.highlight = 0;
  lr->params.output_bps = 16;
  lr->params.user_flip = 0;
  int err = libraw_open_file(lr, path);
  if (!err)
    err = libraw_unpack(lr);
  if (!err)
    err = libraw_dcraw_process(lr);
  libraw_processed_image_t *img = err ? NULL : libraw_dcraw_make_mem_image(lr, &err);
  if (img && (img->type != LIBRAW_IMAGE_BITMAP || img->colors != 3 || img->bits != 16 ||
              img->data_size != (unsigned)img->width * img->height * 3 * 2)) {
    libraw_dcraw_clear_mem(img);
    img = NULL;
    err = DRIP_NOT_BAYER;
  }
  if (img) {
    *width = img->width;
    *height = img->height;
    *rgb = malloc(img->data_size);
    if (*rgb)
      memcpy(*rgb, img->data, img->data_size);
    else
      err = LIBRAW_UNSUFFICIENT_MEMORY;
    libraw_dcraw_clear_mem(img);
  }
  libraw_close(lr);
  return err;
}

void drip_free(void *p) { free(p); }
