package main

import "fmt"

// Base holds shared state.
type Base struct {
	ID int
}

// Server embeds Base and adds a name.
type Server struct {
	Base
	Name string
}

// NewServer builds a Server.
func NewServer(name string) *Server {
	return &Server{Name: name}
}

// Start runs the server.
func (s *Server) Start() error {
	fmt.Println(s.Name)
	return nil
}

// Pick is generic: its own type parameter `T` must NOT become a `uses_type`
// edge, while the real `Server` parameter/return type must.
func Pick[T any](items []T, srv *Server) *Server {
	return srv
}
