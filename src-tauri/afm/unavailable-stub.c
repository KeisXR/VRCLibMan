#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--availability") == 0) {
        fputs(
            "{\"availability\":\"unavailable\",\"variant\":\"\",\"contextSize\":0,\"reason\":\"unsupported-arch\"}\n",
            stdout);
        return 0;
    }
    fputs(
        "{\"ok\":false,\"error\":\"unavailable\",\"message\":\"Apple のオンデバイスモデルは Apple Silicon の macOS 27.2 以上で使えます\"}\n",
        stdout);
    return 0;
}
