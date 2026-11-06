#include <stdbool.h>
#include <stdint.h>
char *qck_start(const char *source, int32_t (*callback)(const char *));
bool qck_dispatch(uint32_t id, uint32_t pid);
void qck_free(char *ptr);
