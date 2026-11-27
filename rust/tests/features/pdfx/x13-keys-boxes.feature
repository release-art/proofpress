Feature: PDF/X-1a and PDF/X-3, keys, boxes, annotations, viewer preferences and fonts
  ISO 15930-1, -3, -4 and -6: the rows that differ from X-4 in what they allow.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-<level>
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | level | check |
      | x13-tr2-default | 1a:2001 | x13.extgstate.no-tr2 |
      | x13-attachment | 3:2003 | x13.filespec.absent |
      | x1a-attachment | 1a:2001 | x1a.filespec.embedded-file |
      | x13-trim-outside-crop | 3:2002 | x13.boxes.trim-within-crop |
      | x13-printermark-inside-bleed | 1a:2001 | x13.annot.outside-bleed-trapnet-only |
      | x13-print-area-trim | 1a:2003 | x13.viewer-prefs.areas |
      | x13-font-restricted | 3:2002 | x13.font.embedding-permitted |
