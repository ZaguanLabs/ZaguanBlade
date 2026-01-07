import React from 'react';
import { TodoItem as TodoItemType } from '../types/events';
import './TodoList.css';

interface TodoItemProps {
  item: TodoItemType;
}

const TodoItem: React.FC<TodoItemProps> = ({ item }) => {
  const icon = item.status === 'completed' ? '✓' : item.status === 'in_progress' ? '⟳' : '';
  const text = item.status === 'in_progress' ? item.activeForm : item.content;
  
  return (
    <div className={`todo-item todo-${item.status}`}>
      <span className="todo-icon">{icon}</span>
      <span className="todo-text">{text}</span>
    </div>
  );
};

interface TodoListProps {
  todos: TodoItemType[];
}

export const TodoList: React.FC<TodoListProps> = ({ todos }) => {
  if (!todos || todos.length === 0) {
    return null;
  }

  return (
    <div className="todo-list">
      {todos.map((todo, index) => (
        <TodoItem key={index} item={todo} />
      ))}
    </div>
  );
};
