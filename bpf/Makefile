CC=clang-15
CFLAGS=-O2 -g -Wall -target bpf
OBJ=pacer_kern.o

%.o: %.c
	$(CC) -c -o $@ $< $(CFLAGS)

all: $(OBJ)