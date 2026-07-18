package cache

import "context"

type Base interface {
	Ping(context.Context) error
}

type Cache interface {
	Base
	Set(string, []byte) error
}
