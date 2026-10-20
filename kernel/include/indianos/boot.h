#ifndef INDIANOS_BOOT_H
#define INDIANOS_BOOT_H

#include <stdbool.h>

/* Boot protocols are private to this module, not kernel-wide interfaces. */
bool boot_validate(void);
void boot_draw_status(void);

#endif
