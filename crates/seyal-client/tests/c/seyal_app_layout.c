#include "SeyalApp.h"

#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>

#define REQUIRE(cond)                                                          \
    do {                                                                       \
        if (!(cond)) {                                                         \
            fprintf(stderr, "layout check failed: %s\n", #cond);               \
            return 1;                                                          \
        }                                                                      \
    } while (0)

int main(void) {
    REQUIRE(SEYAL_APP_ABI_VERSION == 1);
    REQUIRE(sizeof(SeyalAppAction) == 120);
    REQUIRE(offsetof(SeyalAppAction, version) == 0);
    REQUIRE(offsetof(SeyalAppAction, payload) == 104);
    REQUIRE(sizeof(SeyalAppSnapshot) == 112);
    REQUIRE(offsetof(SeyalAppSnapshot, output_utf8) == 80);
    REQUIRE(offsetof(SeyalAppSnapshot, recovery_generation) == 104);
    REQUIRE(sizeof(SeyalAppAxNode) == 72);
    REQUIRE(sizeof(SeyalAppAccessibility) == 24);
    REQUIRE(sizeof(SeyalAppComposer) == 40);
    REQUIRE(sizeof(SeyalAppChrome) == 24);
    REQUIRE(sizeof(SeyalAppTheme) == 36);
    REQUIRE(sizeof(SeyalAppShell) == 64);
    REQUIRE(sizeof(SeyalAppRow) == 56);
    REQUIRE(sizeof(SeyalAppAction) != 0);
    puts("seyal_app_layout ok");
    return 0;
}
