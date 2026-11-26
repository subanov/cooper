-- Requesters belong to a Department (FR-IAM-011); Forms audiences use it (FR-FRM-002).
ALTER TABLE requesters ADD COLUMN department_id uuid REFERENCES departments (id);
CREATE INDEX requesters_department_id_idx ON requesters (department_id);
