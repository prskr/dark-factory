-- Question kinds (specs/agent-requests/spec.md, "Stored questions migrated"). A question with
-- suggested answers becomes a single_choice question with the options a1..aN in stored order
-- and Other allowed; its answer becomes the first option with the same label, else Other.
-- A question without suggested answers becomes a text question. Rows are not re-validated:
-- one option, duplicate labels and long Other answers are read as stored.
UPDATE agent_requests
SET payload = jsonb_build_object(
        'text', payload ->> 'text',
        'kind', 'single_choice',
        'options', (
            SELECT jsonb_agg(
                jsonb_build_object('id', 'a' || s.ord, 'label', s.label) ORDER BY s.ord
            )
            FROM jsonb_array_elements_text(payload -> 'suggested_answers')
                WITH ORDINALITY AS s (label, ord)
        ),
        'allow_other', true,
        'min_selected', NULL,
        'max_selected', NULL
    ),
    decision = CASE
        WHEN decision ->> 'answer' IS NULL THEN decision
        ELSE jsonb_build_object('answer', (
            SELECT CASE
                WHEN m.ord IS NULL THEN jsonb_build_object(
                    'selected', '[]'::jsonb, 'other', decision ->> 'answer'
                )
                ELSE jsonb_build_object(
                    'selected', jsonb_build_array('a' || m.ord), 'other', NULL
                )
            END
            FROM (
                SELECT min(s.ord) AS ord
                FROM jsonb_array_elements_text(payload -> 'suggested_answers')
                    WITH ORDINALITY AS s (label, ord)
                WHERE s.label = decision ->> 'answer'
            ) AS m
        ))
    END
WHERE kind = 'question'
  AND jsonb_array_length(COALESCE(payload -> 'suggested_answers', '[]'::jsonb)) > 0;

UPDATE agent_requests
SET payload = jsonb_build_object(
        'text', payload ->> 'text',
        'kind', 'text',
        'options', '[]'::jsonb,
        'allow_other', false,
        'min_selected', NULL,
        'max_selected', NULL
    ),
    decision = CASE
        WHEN decision ->> 'answer' IS NULL THEN decision
        ELSE jsonb_build_object('answer', jsonb_build_object('text', decision ->> 'answer'))
    END
WHERE kind = 'question' AND NOT (payload ? 'kind');
