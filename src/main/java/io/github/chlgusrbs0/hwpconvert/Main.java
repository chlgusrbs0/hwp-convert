package io.github.chlgusrbs0.hwpconvert;

import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;

public class Main {
    public static void main(String[] args) {
        if (args.length == 0) {
            printUsage();
            return;
        }

        if (args.length != 3) {
            System.out.println("오류: 인자 형식이 올바르지 않습니다.");
            printUsage();
            return;
        }

        String inputFile = args[0];
        String option = args[1];
        String format = args[2];

        if (!option.equals("--to")) {
            System.out.println("오류: 두 번째 인자는 --to 여야 합니다.");
            printUsage();
            return;
        }

        if (!format.equals("txt")) {
            System.out.println("오류: 현재는 txt 형식만 지원합니다.");
            return;
        }

        Path inputPath = Path.of(inputFile);

        if (!Files.exists(inputPath)) {
            System.out.println("오류: 입력 파일을 찾을 수 없습니다.");
            System.out.println("입력 경로: " + inputPath);
            return;
        }

        Path outputPath = createOutputPath(inputPath, format);

        try {
            writeTxtOutput(inputPath, outputPath, format);
        } catch (IOException e) {
            System.out.println("오류: 출력 파일을 생성하지 못했습니다.");
            System.out.println("상세 정보: " + e.getMessage());
            return;
        }

        System.out.println("hwp-convert 실행");
        System.out.println("입력 파일: " + inputPath);
        System.out.println("출력 형식: " + format);
        System.out.println("출력 파일: " + outputPath);
        System.out.println("변환 완료");
    }

    private static Path createOutputPath(Path inputPath, String format) {
        String fileName = inputPath.getFileName().toString();
        int dotIndex = fileName.lastIndexOf('.');

        String baseName;
        if (dotIndex == -1) {
            baseName = fileName;
        } else {
            baseName = fileName.substring(0, dotIndex);
        }

        return inputPath.resolveSibling(baseName + "." + format);
    }

    private static void writeTxtOutput(Path inputPath, Path outputPath, String format) throws IOException {
        StringBuilder content = new StringBuilder();

        content.append("hwp-convert 변환 결과\n");
        content.append("입력 파일: ").append(inputPath.getFileName()).append("\n");
        content.append("출력 형식: ").append(format).append("\n\n");

        String previewText = readPreviewText(inputPath);

        if (previewText == null || previewText.isBlank()) {
            content.append("오류: Preview/PrvText.txt 파일을 찾지 못했거나 내용이 비어 있습니다.\n");
        } else {
            content.append(previewText);
        }

        Files.writeString(outputPath, content.toString());
    }

    private static String readPreviewText(Path inputPath) throws IOException {
        try (InputStream fileInputStream = Files.newInputStream(inputPath);
             ZipInputStream zipInputStream = new ZipInputStream(fileInputStream)) {

            ZipEntry entry;

            while ((entry = zipInputStream.getNextEntry()) != null) {
                if (entry.getName().equals("Preview/PrvText.txt")) {
                    byte[] bytes = zipInputStream.readAllBytes();
                    zipInputStream.closeEntry();
                    return new String(bytes, StandardCharsets.UTF_8);
                }

                zipInputStream.closeEntry();
            }
        }

        return null;
    }

    private static void printUsage() {
        System.out.println("사용법:");
        System.out.println("  hwp-convert <입력 파일> --to <출력 형식>");
        System.out.println();
        System.out.println("예시:");
        System.out.println("  hwp-convert sample.hwpx --to txt");
    }
}