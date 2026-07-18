package cache

import "context"

type DiskStore struct{}

func (d *DiskStore) Ping(ctx context.Context) error { return nil }

func (d *DiskStore) Set(key string, value []byte) error { return nil }
