// Package store keeps tasks.
package store

import (
	"fmt"
	str "strings"
	"github.com/acme/app/internal/model"
	"github.com/gin-gonic/gin"
)

// Store keeps tasks in memory.
type Store struct {
	items []model.Task
	path  string
}

// Saver saves.
type Saver interface {
	Save(force bool) error
}

type ID = string

// New makes a store.
func New(path string) *Store { return &Store{path: path} }

// Save writes all tasks.
func (s *Store) Save(force bool) (int, error) {
	s.flush()
	fmt.Println(str.ToUpper(s.path))
	r := gin.Default()
	_ = r
	return len(s.items), nil
}

func TestSave(t *testing.T) { New("a").Save(true) }
