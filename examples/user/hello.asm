; nasm -f bin examples/user/hello.asm -o alice.hello.kr3
bits 64
default rel
db 'KR3', 1

    mov eax, 1                      ; write
    lea rbx, [rel greeting]
    mov esi, greeting_end - greeting
    int 0x80
    xor eax, eax                    ; exit
    xor ebx, ebx                    ; status
    int 0x80
    ud2

greeting: db 'Hello from CPU ring 3.', 10
greeting_end:
