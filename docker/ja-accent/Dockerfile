FROM ubuntu:latest AS source

RUN apt-get update && apt-get install -y git

RUN git clone --depth 1 https://github.com/kinoh/ja-accent.git /ja-accent

FROM ubuntu:latest AS build

RUN apt-get update && apt-get install -y build-essential

WORKDIR /app

COPY --from=source /ja-accent/packages/CRF++-0.58.tar.gz ./
RUN tar -xzf CRF++-0.58.tar.gz && \
    cd /app/CRF++-0.58 && \
    sh ./configure && \
    make && \
    make install && \
    make clean

COPY --from=source /ja-accent/packages/mecab-0.996.tar.gz ./
RUN tar -xzf mecab-0.996.tar.gz && \
    cd /app/mecab-0.996 && \
    ./configure && \
    make && \
    make install && \
    make clean

FROM python:3.13 AS runtime

WORKDIR /usr/src/app

RUN apt-get update && apt-get install -y unzip

# By downloading, you agree to the UniDic license terms.
# See https://clrd.ninjal.ac.jp/unidic/download.html#unidic_csj
RUN curl -o unidic-csj-202512.zip https://clrd.ninjal.ac.jp/unidic_archive/2512/unidic-csj-202512.zip && \
    unzip unidic-csj-202512.zip -d /usr/src/app/unidic && \
    rm unidic-csj-202512.zip

COPY --from=source /ja-accent/requirements.txt ./
RUN pip install --no-cache-dir -r requirements.txt

ENV LD_LIBRARY_PATH="/usr/local/lib"

COPY --from=build /usr/local/bin/crf_test /usr/local/bin/
COPY --from=build /usr/local/bin/mecab /usr/local/bin/
COPY --from=build /usr/local/lib/libcrfpp.so* /usr/local/lib/
COPY --from=build /usr/local/lib/libmecab.so* /usr/local/lib/
COPY --from=build /usr/local/etc/mecabrc /usr/local/etc/mecabrc
COPY --from=build /usr/local/libexec/mecab/mecab-dict-index /usr/local/libexec/mecab/mecab-dict-index

COPY --from=source /ja-accent/src/user.dic ./
COPY --from=source /ja-accent/src/model_accent ./

COPY --from=source /ja-accent/src/*.py .

ENV MECAB_DICDIR="/usr/src/app/unidic"
ENV MECAB_USERDIC="/usr/src/app/user.dic"

EXPOSE 2954

CMD ["uvicorn", "server:app", "--host", "0.0.0.0", "--port", "2954", "--no-access-log"]
