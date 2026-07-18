package cache

import "context"

type FakeCache struct{}

func (FakeCache) Ping(ctx context.Context) error { return nil }

func (FakeCache) Set(key string, value []byte) error { return nil }
