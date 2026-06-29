#include <stdio.h>

#define MAX_LEN 256
#define SQUARE(x) ((x) * (x))

typedef int handle_t;
typedef int (*compare_fn)(int, int);

struct Point {
    int x;
    int y;
};

union Value {
    int i;
    float f;
};

enum Color {
    RED,
    GREEN,
    BLUE,
};

typedef struct Node {
    int value;
    struct Node *next;
} Node;

typedef struct {
    int width;
    int height;
} Size;

struct Point make_point(int x, int y);

static int add(int a, int b) {
    return a + b;
}

char *clone_string(const char *src);

struct Point usage_global;
static const int LIMIT = 10;

namespace geom {

class Shape {
public:
    Shape();
    virtual ~Shape();
    virtual double area() const;
    int id() { return id_; }
private:
    int id_;
    static int count_;
};

enum class Axis { X, Y, Z };

}  // namespace geom

double geom::Shape::area() const {
    return 0.0;
}
