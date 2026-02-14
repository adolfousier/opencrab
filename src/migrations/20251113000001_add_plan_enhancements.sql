-- Migration to enhance Plan Mode with additional fields
-- Adds test_strategy and technical_stack to plans table
-- Adds acceptance_criteria to plan_tasks table

-- ==================================================
-- Enhance Plans Table
-- ==================================================

-- Add testing strategy field to plans
ALTER TABLE plans ADD COLUMN test_strategy TEXT NOT NULL DEFAULT '';

-- Add technical stack field to plans (JSON array of strings)
ALTER TABLE plans ADD COLUMN technical_stack TEXT NOT NULL DEFAULT '[]';

-- ==================================================
-- Enhance Plan Tasks Table
-- ==================================================

-- Add acceptance criteria field to plan_tasks (JSON array of strings)
ALTER TABLE plan_tasks ADD COLUMN acceptance_criteria TEXT NOT NULL DEFAULT '[]';
