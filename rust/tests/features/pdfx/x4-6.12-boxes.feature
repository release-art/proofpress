Feature: PDF/X-4 6.12, bounding boxes
  Each page has a TrimBox or an ArtBox, not both, preferably the TrimBox; trim or art lies within the bleed, every box
  within the crop box, and every box within the media box.

  Scenario Outline: One defect raises its check on page 1 and nothing else
    Given the fixture "<fixture>"
    When it is verified as PDF/X-4
    Then check "<check>" fails on page 1
    And no check fails but "<check>"

    Examples:
      | fixture | check |
      | x4-no-trim-or-art | x4.boxes.trim-or-art |
      | x4-trim-and-art | x4.boxes.not-both |
      | x4-art-not-trim | x4.boxes.prefer-trim |
      | x4-trim-beyond-bleed | x4.boxes.within-bleed |
      | x4-bleed-beyond-crop | x4.boxes.within-crop |
      | x4-crop-beyond-media | x4.boxes.within-media |
