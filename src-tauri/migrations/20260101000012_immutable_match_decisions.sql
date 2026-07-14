CREATE TRIGGER IF NOT EXISTS immutable_match_decisions
BEFORE UPDATE ON match_decisions
BEGIN
    SELECT RAISE(ABORT, 'match_decisions is immutable');
END;
