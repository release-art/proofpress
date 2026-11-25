Feature: PDF/X-4 6.17 and 6.9, annotations and trap networks
  Annotations sit outside the bleed, printer's marks outside the trim; a TrapNet annotation needs pdf:Trapped True,
  no font fauxing, and appearances in the intent's colour space.

  Scenario Outline: One defect raises its check and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-annot-inside-bleed | x4.annot.outside-bleed |
      | x4-printermark-inside-trim | x4.annot.printermark-outside-trim |
      | x4-trapnet-untrapped | x4.trapped.trapnet-implies-true |
      | x4-trapnet-font-fauxing | x4.trapnet.font-fauxing-empty |
      | x4-trapnet-pcm-rgb | x4.trapnet.pcm-matches-intent |
